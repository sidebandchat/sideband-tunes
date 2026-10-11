use crate::{
    error::TunesError,
    model::{MediaSession, PlatformCapabilities, SessionChangeEvent},
};
use tokio::sync::{mpsc, watch};

pub async fn sessions() -> Result<Vec<MediaSession>, TunesError> {
    Ok(Vec::new())
}
pub fn capabilities() -> PlatformCapabilities {
    PlatformCapabilities {
        platform: std::env::consts::OS.into(),
        supported: false,
        availability: crate::model::PlatformAvailability::Unsupported,
        session_enumeration: false,
        event_notifications: false,
        artwork: false,
        track_ids: false,
        track_uris: false,
        position_notifications: false,
        notes: vec!["Unsupported operating system".into()],
    }
}
pub async fn watch(
    _sender: mpsc::Sender<SessionChangeEvent>,
    mut cancel: watch::Receiver<bool>,
) -> Result<(), TunesError> {
    while !*cancel.borrow() && cancel.changed().await.is_ok() {}
    Ok(())
}
