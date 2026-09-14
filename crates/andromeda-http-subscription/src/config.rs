//! Streaming subscription transport configuration.

use serde::{Deserialize, Serialize};
use thiserror::Error;
use toml_comment::TomlComment;

/// Invalid streaming subscription transport configuration.
#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum NotificationConfigError {
    /// SSE channel capacity was configured as zero.
    #[error("notifications.streaming.sse.channel_buffer_size must be at least 1")]
    ZeroSseChannelCapacity,
    /// SSE keep-alive cadence was configured as zero.
    #[error("notifications.streaming.sse.keep_alive_interval_secs must be at least 1")]
    ZeroSseKeepAliveInterval,
    /// WebSocket channel capacity was configured as zero.
    #[error("notifications.streaming.ws.channel_buffer_size must be at least 1")]
    ZeroWebSocketChannelCapacity,
    /// WebSocket ping cadence was configured as zero.
    #[error("notifications.streaming.ws.ping_interval_secs must be at least 1")]
    ZeroWebSocketPingInterval,
}

/// Server-Sent Events transport limits.
#[derive(Deserialize, Serialize, TomlComment, Debug, Clone)]
pub struct SseConfig {
    /// Per-connection notification channel capacity.
    pub channel_buffer_size: usize,
    /// Keep-alive interval for idle connections.
    pub keep_alive_interval_secs: u64,
}

impl Default for SseConfig {
    fn default() -> SseConfig {
        SseConfig {
            channel_buffer_size: 64,
            keep_alive_interval_secs: 15,
        }
    }
}

/// WebSocket transport limits.
#[derive(Deserialize, Serialize, TomlComment, Debug, Clone)]
pub struct WsConfig {
    /// Per-connection notification channel capacity.
    pub channel_buffer_size: usize,
    /// Ping interval for idle connections.
    pub ping_interval_secs: u64,
}

impl Default for WsConfig {
    fn default() -> WsConfig {
        WsConfig {
            channel_buffer_size: 64,
            ping_interval_secs: 30,
        }
    }
}

/// Streaming transport configuration.
#[derive(Deserialize, Serialize, Default, TomlComment, Debug, Clone)]
pub struct NotificationConfig {
    /// Server-Sent Events settings.
    pub sse: SseConfig,
    /// WebSocket settings.
    pub ws: WsConfig,
}

impl NotificationConfig {
    /// Validate SSE and WebSocket delivery limits.
    ///
    /// # Errors
    ///
    /// Returns [`NotificationConfigError`] when any of the four limits is
    /// configured as zero: an SSE or WebSocket channel capacity of zero can hold
    /// no notification, and a keep-alive or ping interval of zero leaves an idle
    /// connection with no liveness traffic at all.
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
    use pretty_assertions::assert_eq;

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
