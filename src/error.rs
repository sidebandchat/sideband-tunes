#[derive(Debug, thiserror::Error)]
pub enum TunesError {
    #[error("system media sessions are unsupported on this platform")]
    Unsupported,
    #[error("system media API is unavailable: {0}")]
    Unavailable(String),
    #[error("permission to observe system media sessions was denied: {0}")]
    PermissionDenied(String),
    #[error("system media backend failed: {0}")]
    Backend(String),
}

impl From<TunesError> for napi::Error {
    fn from(value: TunesError) -> Self {
        napi::Error::from_reason(value.to_string())
    }
}
