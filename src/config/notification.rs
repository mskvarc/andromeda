//! Streaming notification transport configuration.

use confique::Config;
use serde::Serialize;

/// Server-Sent Events transport limits.
#[derive(Config, Debug, Clone, Serialize)]
pub struct SseConfig {
    /// Per-connection notification channel capacity.
    #[config(env = "ANDROMEDA_NOTIFICATIONS_SSE_CHANNEL_BUFFER_SIZE", default = 64)]
    pub channel_buffer_size: usize,
    /// Keep-alive interval for idle connections.
    #[config(env = "ANDROMEDA_NOTIFICATIONS_SSE_KEEP_ALIVE_INTERVAL_SECS", default = 15)]
    pub keep_alive_interval_secs: u64,
}

/// WebSocket transport limits.
#[derive(Config, Debug, Clone, Serialize)]
pub struct WsConfig {
    /// Per-connection notification channel capacity.
    #[config(env = "ANDROMEDA_NOTIFICATIONS_WS_CHANNEL_BUFFER_SIZE", default = 64)]
    pub channel_buffer_size: usize,
    /// Ping interval for idle connections.
    #[config(env = "ANDROMEDA_NOTIFICATIONS_WS_PING_INTERVAL_SECS", default = 30)]
    pub ping_interval_secs: u64,
}

/// Streaming transport configuration.
#[derive(Config, Debug, Clone, Serialize)]
pub struct NotificationConfig {
    /// Server-Sent Events settings.
    #[config(nested)]
    pub sse: SseConfig,
    /// WebSocket settings.
    #[config(nested)]
    pub ws: WsConfig,
}
