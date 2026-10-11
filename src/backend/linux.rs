use std::collections::{HashMap, HashSet};

use futures_util::StreamExt;
use tokio::{
    sync::{mpsc, watch},
    task::JoinSet,
};
use zbus::{
    Connection,
    fdo::DBusProxy,
    zvariant::{ObjectPath, OwnedValue},
};

use crate::{
    error::TunesError,
    model::{
        ChangeKind, MediaSession, PlatformCapabilities, PlatformValue, PlaybackState,
        SessionChangeEvent,
    },
};

const PREFIX: &str = "org.mpris.MediaPlayer2.";
const PATH: &str = "/org/mpris/MediaPlayer2";

#[zbus::proxy(
    interface = "org.mpris.MediaPlayer2",
    default_path = "/org/mpris/MediaPlayer2"
)]
trait Root {
    #[zbus(property)]
    fn identity(&self) -> zbus::Result<String>;
    #[zbus(property)]
    fn desktop_entry(&self) -> zbus::Result<String>;
}

#[zbus::proxy(
    interface = "org.mpris.MediaPlayer2.Player",
    default_path = "/org/mpris/MediaPlayer2"
)]
trait Player {
    #[zbus(property)]
    fn playback_status(&self) -> zbus::Result<String>;
    #[zbus(property)]
    fn metadata(&self) -> zbus::Result<HashMap<String, OwnedValue>>;
    #[zbus(property)]
    fn position(&self) -> zbus::Result<i64>;
    #[zbus(signal)]
    fn seeked(&self, position: i64) -> zbus::Result<()>;
}

pub fn capabilities() -> PlatformCapabilities {
    PlatformCapabilities {
        platform: "linux".into(),
        supported: true,
        availability: crate::model::PlatformAvailability::Available,
        session_enumeration: true,
        event_notifications: true,
        artwork: true,
        track_ids: true,
        track_uris: true,
        // MPRIS intentionally does not signal continuously advancing position.
        position_notifications: false,
        notes: vec!["Position is sampled on discovery and updated on seek/property changes".into()],
    }
}

pub async fn sessions() -> Result<Vec<MediaSession>, TunesError> {
    let connection = Connection::session().await.map_err(map_zbus)?;
    let names = mpris_names(&connection).await?;
    let mut result = Vec::with_capacity(names.len());
    for name in names {
        if let Ok(session) = read_session(&connection, &name).await {
            result.push(session);
        }
    }
    Ok(result)
}

pub async fn watch(
    sender: mpsc::Sender<SessionChangeEvent>,
    mut cancel: watch::Receiver<bool>,
) -> Result<(), TunesError> {
    let connection = Connection::session().await.map_err(map_zbus)?;
    let dbus = DBusProxy::new(&connection).await.map_err(map_zbus)?;
    let mut owners = dbus.receive_name_owner_changed().await.map_err(map_zbus)?;
    let mut tasks = JoinSet::new();
    let mut active = HashSet::new();

    for name in mpris_names(&connection).await? {
        active.insert(name.clone());
        spawn_player(
            &mut tasks,
            connection.clone(),
            name,
            sender.clone(),
            cancel.clone(),
        );
    }
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
            signal = owners.next() => {
                let Some(signal) = signal else { return Err(TunesError::Unavailable("D-Bus owner stream ended".into())); };
                let args = signal.args().map_err(map_zbus)?;
                let name = args.name().to_string();
                if !name.starts_with(PREFIX) { continue; }
                if args.new_owner().is_some() {
                    if args.old_owner().is_some() && active.remove(&name) {
                        let _ = sender.send(SessionChangeEvent { kind: ChangeKind::Removed, session_id: Some(name.clone()), session: None, message: None }).await;
                    }
                    if active.insert(name.clone()) {
                        spawn_player(&mut tasks, connection.clone(), name, sender.clone(), cancel.clone());
                    }
                } else if active.remove(&name) {
                    let _ = sender.send(SessionChangeEvent { kind: ChangeKind::Removed, session_id: Some(name), session: None, message: None }).await;
                }
            }
        }
    }
    tasks.abort_all();
    Ok(())
}

fn spawn_player(
    tasks: &mut JoinSet<()>,
    connection: Connection,
    name: String,
    sender: mpsc::Sender<SessionChangeEvent>,
    mut cancel: watch::Receiver<bool>,
) {
    tasks.spawn(async move {
        let first = match read_session(&connection, &name).await {
            Ok(value) => value,
            Err(_) => return,
        };
        if sender
            .send(SessionChangeEvent {
                kind: ChangeKind::Added,
                session_id: Some(name.clone()),
                session: Some(first),
                message: None,
            })
            .await
            .is_err()
        {
            return;
        }
        let proxy = match PlayerProxy::builder(&connection)
            .destination(name.as_str())
            .and_then(|builder| builder.path(PATH))
            .map(|builder| builder.build())
        {
            Ok(future) => match future.await {
                Ok(proxy) => proxy,
                Err(_) => return,
            },
            Err(_) => return,
        };
        let mut metadata = proxy.receive_metadata_changed().await;
        let mut playback = proxy.receive_playback_status_changed().await;
        let mut seeked = match proxy.receive_seeked().await {
            Ok(stream) => stream,
            Err(_) => return,
        };
        loop {
            tokio::select! {
                _ = cancel.changed() => if *cancel.borrow() { break; },
                value = metadata.next() => if value.is_none() { break; },
                value = playback.next() => if value.is_none() { break; },
                value = seeked.next() => if value.is_none() { break; },
            }
            if let Ok(session) = read_session(&connection, &name).await
                && sender
                    .send(SessionChangeEvent {
                        kind: ChangeKind::Changed,
                        session_id: Some(name.clone()),
                        session: Some(session),
                        message: None,
                    })
                    .await
                    .is_err()
            {
                break;
            }
        }
    });
}

async fn mpris_names(connection: &Connection) -> Result<Vec<String>, TunesError> {
    let proxy = DBusProxy::new(connection).await.map_err(map_zbus)?;
    let names = proxy.list_names().await.map_err(map_zbus)?;
    Ok(names
        .into_iter()
        .map(|name| name.to_string())
        .filter(|name| name.starts_with(PREFIX))
        .collect())
}

async fn read_session(connection: &Connection, name: &str) -> Result<MediaSession, TunesError> {
    let root = RootProxy::builder(connection)
        .destination(name)
        .map_err(map_zbus)?
        .build()
        .await
        .map_err(map_zbus)?;
    let player = PlayerProxy::builder(connection)
        .destination(name)
        .map_err(map_zbus)?
        .build()
        .await
        .map_err(map_zbus)?;
    let metadata = player.metadata().await.unwrap_or_default();
    let status = player
        .playback_status()
        .await
        .unwrap_or_else(|_| "Unknown".into());
    let position = player.position().await.ok().and_then(micros_to_millis);
    let identity = root.identity().await.ok();
    let desktop_entry = root.desktop_entry().await.ok();

    let string = |key: &str| metadata.get(key).and_then(value_string);
    let artists = metadata
        .get("xesam:artist")
        .and_then(value_strings)
        .map(|v| v.join(", "));
    let duration_ms = metadata
        .get("mpris:length")
        .and_then(value_i64)
        .and_then(micros_to_millis);
    let mut platform_values = Vec::new();
    if let Some(entry) = &desktop_entry {
        platform_values.push(PlatformValue {
            key: "desktopEntry".into(),
            value: entry.clone(),
        });
    }

    Ok(MediaSession {
        id: name.into(),
        title: string("xesam:title"),
        artist: artists,
        album: string("xesam:album"),
        artwork_url: string("mpris:artUrl"),
        duration_ms,
        position_ms: position,
        playback_state: match status.as_str() {
            "Playing" => PlaybackState::Playing,
            "Paused" => PlaybackState::Paused,
            "Stopped" => PlaybackState::Stopped,
            _ => PlaybackState::Unknown,
        },
        application_id: desktop_entry.unwrap_or_else(|| name.trim_start_matches(PREFIX).into()),
        application_name: identity,
        track_id: metadata.get("mpris:trackid").and_then(value_path),
        track_uri: string("xesam:url"),
        platform: "linux".into(),
        platform_values,
    })
}

fn value_string(value: &OwnedValue) -> Option<String> {
    String::try_from(value.clone()).ok()
}
fn value_strings(value: &OwnedValue) -> Option<Vec<String>> {
    Vec::<String>::try_from(value.clone()).ok()
}
fn value_i64(value: &OwnedValue) -> Option<i64> {
    i64::try_from(value.clone()).ok()
}
fn value_path(value: &OwnedValue) -> Option<String> {
    ObjectPath::try_from(value.clone())
        .ok()
        .map(|v| v.to_string())
}
fn micros_to_millis(value: i64) -> Option<i64> {
    (value >= 0).then_some(value / 1_000)
}
fn map_zbus(error: impl std::fmt::Display) -> TunesError {
    TunesError::Backend(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn converts_only_non_negative_timestamps() {
        assert_eq!(micros_to_millis(1_234_567), Some(1_234));
        assert_eq!(micros_to_millis(-1), None);
    }
}
