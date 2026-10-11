use napi_derive::napi;

#[napi(string_enum = "lowercase")]
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum PlaybackState {
    Playing,
    Paused,
    #[default]
    Stopped,
    Unknown,
}

#[napi(object)]
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PlatformValue {
    pub key: String,
    pub value: String,
}

#[napi(object)]
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MediaSession {
    pub id: String,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub artwork_url: Option<String>,
    pub duration_ms: Option<i64>,
    pub position_ms: Option<i64>,
    pub playback_state: PlaybackState,
    pub application_id: String,
    pub application_name: Option<String>,
    pub track_id: Option<String>,
    pub track_uri: Option<String>,
    pub platform: String,
    pub platform_values: Vec<PlatformValue>,
}

#[napi(string_enum = "lowercase")]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChangeKind {
    Added,
    Changed,
    Removed,
    Reset,
    Error,
}

#[napi(string_enum = "lowercase")]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PlatformAvailability {
    Available,
    Unsupported,
}

#[napi(object)]
#[derive(Clone, Debug)]
pub struct SessionChangeEvent {
    pub kind: ChangeKind,
    pub session_id: Option<String>,
    pub session: Option<MediaSession>,
    pub message: Option<String>,
}

#[napi(object)]
pub struct PlatformCapabilities {
    pub platform: String,
    pub supported: bool,
    pub availability: PlatformAvailability,
    pub session_enumeration: bool,
    pub event_notifications: bool,
    pub artwork: bool,
    pub track_ids: bool,
    pub track_uris: bool,
    pub position_notifications: bool,
    pub notes: Vec<String>,
}
