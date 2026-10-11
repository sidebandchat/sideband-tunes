use tokio::sync::{mpsc, watch};

use crate::{
    error::TunesError,
    model::{MediaSession, PlatformCapabilities, SessionChangeEvent},
};

#[cfg(target_os = "linux")]
mod linux;
#[cfg(not(any(target_os = "linux", target_os = "windows")))]
mod unsupported;
#[cfg(target_os = "windows")]
mod windows;

#[cfg(target_os = "linux")]
use linux as platform;
#[cfg(not(any(target_os = "linux", target_os = "windows")))]
use unsupported as platform;
#[cfg(target_os = "windows")]
use windows as platform;

pub async fn sessions() -> Result<Vec<MediaSession>, TunesError> {
    platform::sessions().await
}

pub fn capabilities() -> PlatformCapabilities {
    platform::capabilities()
}

pub async fn watch(
    sender: mpsc::Sender<SessionChangeEvent>,
    cancel: watch::Receiver<bool>,
) -> Result<(), TunesError> {
    platform::watch(sender, cancel).await
}
