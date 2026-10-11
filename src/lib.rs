mod backend;
pub mod error;
pub mod model;

use std::sync::Arc;

use napi::{
    bindgen_prelude::*,
    threadsafe_function::{ThreadsafeFunction, ThreadsafeFunctionCallMode},
};
use napi_derive::napi;
use tokio::{
    sync::{Mutex, mpsc, watch},
    task::JoinHandle,
};

pub use model::*;

/// Return a point-in-time snapshot of every media session exposed by the platform.
pub async fn sessions() -> std::result::Result<Vec<MediaSession>, error::TunesError> {
    backend::sessions().await
}

/// Describe which normalized features the current platform can provide.
pub fn platform_capabilities() -> PlatformCapabilities {
    backend::capabilities()
}

/// A native Rust stream-like observer. Call [`SessionObserver::recv`] until shutdown,
/// then call [`SessionObserver::close`] for deterministic cleanup.
pub struct SessionObserver {
    receiver: mpsc::Receiver<SessionChangeEvent>,
    cancel: watch::Sender<bool>,
    task: Option<JoinHandle<()>>,
}

impl SessionObserver {
    pub fn new(capacity: usize) -> Self {
        let (sender, receiver) = mpsc::channel(capacity.max(1));
        let (cancel, cancel_receiver) = watch::channel(false);
        let error_sender = sender.clone();
        let task = tokio::spawn(async move {
            if let Err(error) = backend::watch(sender, cancel_receiver).await {
                let _ = error_sender
                    .send(SessionChangeEvent {
                        kind: ChangeKind::Error,
                        session_id: None,
                        session: None,
                        message: Some(error.to_string()),
                    })
                    .await;
            }
        });
        Self {
            receiver,
            cancel,
            task: Some(task),
        }
    }

    pub async fn recv(&mut self) -> Option<SessionChangeEvent> {
        self.receiver.recv().await
    }

    pub async fn close(mut self) {
        self.cancel.send_replace(true);
        if let Some(task) = self.task.take() {
            let _ = task.await;
        }
    }
}

impl Drop for SessionObserver {
    fn drop(&mut self) {
        self.cancel.send_replace(true);
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

#[napi]
pub async fn get_sessions() -> napi::Result<Vec<MediaSession>> {
    sessions().await.map_err(Into::into)
}

#[napi]
pub fn get_platform_capabilities() -> PlatformCapabilities {
    platform_capabilities()
}

#[napi]
pub struct NativeMediaSessionObserver {
    task: Arc<Mutex<Option<JoinHandle<()>>>>,
    cancel: watch::Sender<bool>,
}

impl Default for NativeMediaSessionObserver {
    fn default() -> Self {
        Self::new()
    }
}

#[napi]
impl NativeMediaSessionObserver {
    #[napi(constructor)]
    pub fn new() -> Self {
        let (cancel, _) = watch::channel(false);
        Self {
            task: Arc::new(Mutex::new(None)),
            cancel,
        }
    }

    #[napi]
    pub async fn start(
        &self,
        callback: Arc<
            ThreadsafeFunction<SessionChangeEvent, (), SessionChangeEvent, Status, false>,
        >,
    ) -> napi::Result<()> {
        let mut task = self.task.lock().await;
        if task.is_some() {
            return Ok(());
        }
        self.cancel.send_replace(false);
        let mut cancel = self.cancel.subscribe();
        let (sender, mut receiver) = mpsc::channel(128);
        let callback_task = callback.clone();
        *task = Some(tokio::spawn(async move {
            let backend_cancel = cancel.clone();
            let backend_task = tokio::spawn(async move {
                if let Err(error) = backend::watch(sender.clone(), backend_cancel).await {
                    let _ = sender
                        .send(SessionChangeEvent {
                            kind: ChangeKind::Error,
                            session_id: None,
                            session: None,
                            message: Some(error.to_string()),
                        })
                        .await;
                }
            });
            loop {
                tokio::select! {
                    event = receiver.recv() => {
                        match event {
                            Some(event) => { callback_task.call(event, ThreadsafeFunctionCallMode::NonBlocking); }
                            None => break,
                        }
                    },
                    changed = cancel.changed() => {
                        if changed.is_err() || *cancel.borrow() { break; }
                    }
                }
            }
            backend_task.abort();
        }));
        Ok(())
    }

    #[napi]
    pub async fn stop(&self) {
        self.cancel.send_replace(true);
        if let Some(task) = self.task.lock().await.take() {
            let _ = task.await;
        }
    }
}

impl Drop for NativeMediaSessionObserver {
    fn drop(&mut self) {
        self.cancel.send_replace(true);
        if let Ok(mut task) = self.task.try_lock()
            && let Some(task) = task.take()
        {
            task.abort();
        }
    }
}
