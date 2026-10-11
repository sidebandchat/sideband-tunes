use std::collections::{HashMap, HashSet};

use base64::{Engine, engine::general_purpose::STANDARD};
use tokio::sync::{mpsc, watch};
use windows::{
    Foundation::TypedEventHandler,
    Media::Control::{
        GlobalSystemMediaTransportControlsSession as WinSession,
        GlobalSystemMediaTransportControlsSessionManager as SessionManager,
        GlobalSystemMediaTransportControlsSessionPlaybackStatus as WinPlaybackStatus,
    },
    Storage::Streams::DataReader,
    Win32::System::WinRT::{RO_INIT_MULTITHREADED, RoInitialize, RoUninitialize},
};

use crate::{
    error::TunesError,
    model::{
        ChangeKind, MediaSession, PlatformCapabilities, PlatformValue, PlaybackState,
        SessionChangeEvent,
    },
};

pub fn capabilities() -> PlatformCapabilities {
    PlatformCapabilities {
        platform: "windows".into(),
        supported: true,
        availability: crate::model::PlatformAvailability::Available,
        session_enumeration: true,
        event_notifications: true,
        artwork: true,
        track_ids: false,
        track_uris: false,
        position_notifications: true,
        notes: vec!["GSMTC does not expose a portable track ID or URI".into()],
    }
}

pub async fn sessions() -> Result<Vec<MediaSession>, TunesError> {
    tokio::task::spawn_blocking(|| futures_executor::block_on(sessions_local()))
        .await
        .map_err(|error| TunesError::Backend(error.to_string()))?
}

async fn sessions_local() -> Result<Vec<MediaSession>, TunesError> {
    let _apartment = Apartment::initialize()?;
    let manager = request_manager().await?;
    read_all(&manager).await
}

pub async fn watch(
    sender: mpsc::Sender<SessionChangeEvent>,
    cancel: watch::Receiver<bool>,
) -> Result<(), TunesError> {
    tokio::task::spawn_blocking(move || futures_executor::block_on(watch_local(sender, cancel)))
        .await
        .map_err(|error| TunesError::Backend(error.to_string()))?
}

async fn watch_local(
    sender: mpsc::Sender<SessionChangeEvent>,
    mut cancel: watch::Receiver<bool>,
) -> Result<(), TunesError> {
    let _apartment = Apartment::initialize()?;
    let manager = request_manager().await?;
    let (wake_tx, mut wake_rx) = mpsc::unbounded_channel::<Wake>();
    let session_tx = wake_tx.clone();
    let manager_token = manager
        .SessionsChanged(&TypedEventHandler::new(move |_, _| {
            let _ = session_tx.send(Wake::Sessions);
            Ok(())
        }))
        .map_err(map_windows)?;
    let mut registrations = HashMap::new();
    reconcile(&manager, &wake_tx, &sender, &mut registrations).await?;
    let _ = sender
        .send(SessionChangeEvent {
            kind: ChangeKind::Reset,
            session_id: None,
            session: None,
            message: None,
        })
        .await;

    loop {
        tokio::select! {
            _ = cancel.changed() => if *cancel.borrow() { break; },
            wake = wake_rx.recv() => match wake {
                Some(Wake::Sessions) => reconcile(&manager, &wake_tx, &sender, &mut registrations).await?,
                Some(Wake::Session(id)) => {
                    if let Some(registration) = registrations.get(&id)
                        && let Ok(session) = read_session(&registration.session).await
                    {
                        let _ = sender.send(SessionChangeEvent { kind: ChangeKind::Changed, session_id: Some(id), session: Some(session), message: None }).await;
                    }
                }
                None => break,
            }
        }
    }
    let _ = manager.RemoveSessionsChanged(manager_token);
    for (_, registration) in registrations {
        registration.remove();
    }
    Ok(())
}

struct Apartment(bool);

impl Apartment {
    fn initialize() -> Result<Self, TunesError> {
        match unsafe { RoInitialize(RO_INIT_MULTITHREADED) } {
            Ok(()) => Ok(Self(true)),
            // The worker can be reused by Tokio. An existing apartment is sufficient.
            Err(error) if error.code().0 as u32 == 0x8001_0106 => Ok(Self(false)),
            Err(error) => Err(map_windows(error)),
        }
    }
}

impl Drop for Apartment {
    fn drop(&mut self) {
        if self.0 {
            unsafe { RoUninitialize() };
        }
    }
}

#[derive(Clone)]
enum Wake {
    Sessions,
    Session(String),
}

struct Registration {
    session: WinSession,
    media: i64,
    playback: i64,
    timeline: i64,
}

impl Registration {
    fn remove(self) {
        let _ = self.session.RemoveMediaPropertiesChanged(self.media);
        let _ = self.session.RemovePlaybackInfoChanged(self.playback);
        let _ = self.session.RemoveTimelinePropertiesChanged(self.timeline);
    }
}

async fn reconcile(
    manager: &SessionManager,
    wake: &mpsc::UnboundedSender<Wake>,
    sender: &mpsc::Sender<SessionChangeEvent>,
    registrations: &mut HashMap<String, Registration>,
) -> Result<(), TunesError> {
    let current = manager.GetSessions().map_err(map_windows)?;
    let mut found = HashSet::new();
    for session in current {
        let id = session
            .SourceAppUserModelId()
            .map_err(map_windows)?
            .to_string_lossy();
        found.insert(id.clone());
        if !registrations.contains_key(&id) {
            let media_tx = wake.clone();
            let media_id = id.clone();
            let media = session
                .MediaPropertiesChanged(&TypedEventHandler::new(move |_, _| {
                    let _ = media_tx.send(Wake::Session(media_id.clone()));
                    Ok(())
                }))
                .map_err(map_windows)?;
            let playback_tx = wake.clone();
            let playback_id = id.clone();
            let playback = session
                .PlaybackInfoChanged(&TypedEventHandler::new(move |_, _| {
                    let _ = playback_tx.send(Wake::Session(playback_id.clone()));
                    Ok(())
                }))
                .map_err(map_windows)?;
            let timeline_tx = wake.clone();
            let timeline_id = id.clone();
            let timeline = session
                .TimelinePropertiesChanged(&TypedEventHandler::new(move |_, _| {
                    let _ = timeline_tx.send(Wake::Session(timeline_id.clone()));
                    Ok(())
                }))
                .map_err(map_windows)?;
            let snapshot = read_session(&session).await?;
            registrations.insert(
                id.clone(),
                Registration {
                    session,
                    media,
                    playback,
                    timeline,
                },
            );
            let _ = sender
                .send(SessionChangeEvent {
                    kind: ChangeKind::Added,
                    session_id: Some(id),
                    session: Some(snapshot),
                    message: None,
                })
                .await;
        }
    }
    let removed: Vec<_> = registrations
        .keys()
        .filter(|id| !found.contains(*id))
        .cloned()
        .collect();
    for id in removed {
        if let Some(registration) = registrations.remove(&id) {
            registration.remove();
        }
        let _ = sender
            .send(SessionChangeEvent {
                kind: ChangeKind::Removed,
                session_id: Some(id),
                session: None,
                message: None,
            })
            .await;
    }
    Ok(())
}

async fn request_manager() -> Result<SessionManager, TunesError> {
    SessionManager::RequestAsync()
        .map_err(map_windows)?
        .await
        .map_err(|error| {
            let message = error.to_string();
            if message.contains("denied") || message.contains("0x80070005") {
                TunesError::PermissionDenied(message)
            } else {
                TunesError::Unavailable(message)
            }
        })
}

async fn read_all(manager: &SessionManager) -> Result<Vec<MediaSession>, TunesError> {
    let mut result = Vec::new();
    for session in manager.GetSessions().map_err(map_windows)? {
        if let Ok(snapshot) = read_session(&session).await {
            result.push(snapshot);
        }
    }
    Ok(result)
}

async fn read_session(session: &WinSession) -> Result<MediaSession, TunesError> {
    let id = session
        .SourceAppUserModelId()
        .map_err(map_windows)?
        .to_string_lossy();
    let media = session
        .TryGetMediaPropertiesAsync()
        .map_err(map_windows)?
        .await
        .map_err(map_windows)?;
    let playback = session.GetPlaybackInfo().map_err(map_windows)?;
    let timeline = session.GetTimelineProperties().map_err(map_windows)?;
    let artwork_url = if let Ok(thumbnail) = media.Thumbnail() {
        read_artwork(&thumbnail).await.ok()
    } else {
        None
    };
    let mut platform_values = Vec::new();
    if let Ok(number) = media.TrackNumber() {
        platform_values.push(PlatformValue {
            key: "trackNumber".into(),
            value: number.to_string(),
        });
    }
    if let Ok(subtitle) = media.Subtitle()
        && !subtitle.is_empty()
    {
        platform_values.push(PlatformValue {
            key: "subtitle".into(),
            value: subtitle.to_string_lossy(),
        });
    }
    Ok(MediaSession {
        id: id.clone(),
        title: nonempty(media.Title().ok().map(|v| v.to_string_lossy())),
        artist: nonempty(media.Artist().ok().map(|v| v.to_string_lossy())),
        album: nonempty(media.AlbumTitle().ok().map(|v| v.to_string_lossy())),
        artwork_url,
        duration_ms: timespan_ms(timeline.EndTime().ok()),
        position_ms: timespan_ms(timeline.Position().ok()),
        playback_state: match playback.PlaybackStatus().ok() {
            Some(WinPlaybackStatus::Playing) => PlaybackState::Playing,
            Some(WinPlaybackStatus::Paused) => PlaybackState::Paused,
            Some(WinPlaybackStatus::Stopped) | Some(WinPlaybackStatus::Closed) => {
                PlaybackState::Stopped
            }
            _ => PlaybackState::Unknown,
        },
        application_id: id,
        application_name: None,
        track_id: None,
        track_uri: None,
        platform: "windows".into(),
        platform_values,
    })
}

async fn read_artwork(
    reference: &windows::Storage::Streams::IRandomAccessStreamReference,
) -> Result<String, TunesError> {
    let stream = reference
        .OpenReadAsync()
        .map_err(map_windows)?
        .await
        .map_err(map_windows)?;
    let size = stream.Size().map_err(map_windows)?;
    if size > 8 * 1024 * 1024 {
        return Err(TunesError::Backend(
            "artwork exceeds the 8 MiB safety limit".into(),
        ));
    }
    let reader = DataReader::CreateDataReader(&stream).map_err(map_windows)?;
    reader
        .LoadAsync(size as u32)
        .map_err(map_windows)?
        .await
        .map_err(map_windows)?;
    let mut bytes = vec![0; size as usize];
    reader.ReadBytes(&mut bytes).map_err(map_windows)?;
    let content_type = stream.ContentType().map_err(map_windows)?.to_string_lossy();
    Ok(format!(
        "data:{};base64,{}",
        content_type,
        STANDARD.encode(bytes)
    ))
}

fn timespan_ms(value: Option<windows::Foundation::TimeSpan>) -> Option<i64> {
    value.and_then(|v| (v.Duration >= 0).then_some(v.Duration / 10_000))
}
fn nonempty(value: Option<String>) -> Option<String> {
    value.filter(|value| !value.is_empty())
}
fn map_windows(error: windows::core::Error) -> TunesError {
    TunesError::Backend(error.to_string())
}
