//! Streaming subscription transport configuration.

use confique::Config;
use serde::Serialize;
use thiserror::Error;

/// Invalid streaming subscription transport configuration.
#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum NotificationConfigError {
    /// SSE channel capacity was configured as zero.
    #[error("notifications.sse.channel_buffer_size must be at least 1")]
    ZeroSseChannelCapacity,
    /// SSE keep-alive cadence was configured as zero.
    #[error("notifications.sse.keep_alive_interval_secs must be at least 1")]
    ZeroSseKeepAliveInterval,
    /// WebSocket channel capacity was configured as zero.
    #[error("notifications.ws.channel_buffer_size must be at least 1")]
    ZeroWebSocketChannelCapacity,
    /// WebSocket ping cadence was configured as zero.
    #[error("notifications.ws.ping_interval_secs must be at least 1")]
    ZeroWebSocketPingInterval,
}

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

impl NotificationConfig {
    /// Validate SSE and WebSocket delivery limits.
    pub const fn validate(&self) -> Result<(), NotificationConfigError> {
        if self.sse.channel_buffer_size == 0 {
            return Err(NotificationConfigError::ZeroSseChannelCapacity);
        }
        if self.sse.keep_alive_interval_secs == 0 {
            return Err(NotificationConfigError::ZeroSseKeepAliveInterval);
        }
        if self.ws.channel_buffer_size == 0 {
            return Err(NotificationConfigError::ZeroWebSocketChannelCapacity);
        }
        if self.ws.ping_interval_secs == 0 {
            return Err(NotificationConfigError::ZeroWebSocketPingInterval);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{NotificationConfig, NotificationConfigError, SseConfig, WsConfig};

    #[test]
    fn zero_websocket_ping_interval_is_rejected() {
        let config = NotificationConfig {
            sse: SseConfig {
                channel_buffer_size: 64,
                keep_alive_interval_secs: 15,
            },
            ws: WsConfig {
                channel_buffer_size: 64,
                ping_interval_secs: 0,
            },
        };
        assert_eq!(config.validate(), Err(NotificationConfigError::ZeroWebSocketPingInterval));
    }
}
