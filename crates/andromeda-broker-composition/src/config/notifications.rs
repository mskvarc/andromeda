//! Composition of the durable-delivery and streaming notification configurations.

use andromeda_configuration_scalars::human_duration::HumanDuration;
use andromeda_http_subscription::config::{NotificationConfig, NotificationConfigError};
use andromeda_notification::delivery_config::{DeliveryConfig, DeliveryConfigError};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use toml_comment::TomlComment;

/// Invalid notification configuration.
#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum NotificationsConfigError {
    /// The durable delivery configuration was rejected by its owner.
    #[error(transparent)]
    Delivery(#[from] DeliveryConfigError),
    /// The streaming transport configuration was rejected by its owner.
    #[error(transparent)]
    Streaming(#[from] NotificationConfigError),
    /// The interval-notification idle sleep ceiling was configured as zero.
    #[error("notifications.interval_max_sleep must be greater than zero")]
    ZeroIntervalMaxSleep,
}

/// Durable-delivery and streaming notification configuration.
///
/// The two sub-configurations are unrelated in what they own -- one governs the
/// durable HTTP/MQTT delivery queue, the other the SSE and WebSocket streaming
/// transports -- but both are one operator-facing `notifications` section.
#[derive(Deserialize, Serialize, TomlComment, Debug, Clone)]
pub struct NotificationsConfig {
    /// Durable notification event and delivery configuration.
    pub delivery: DeliveryConfig,
    /// Streaming (Server-Sent Events and WebSocket) transport configuration.
    pub streaming: NotificationConfig,
    /// Ceiling the periodic (`timeInterval`) notification drain's idle backoff
    /// reaches while no subscription change wakes it.
    ///
    /// The drain sleeps until the next subscription is actually due (clause
    /// 5.8.6, floor 1 s); this only bounds how long it waits when nothing is due,
    /// so a change to the interval subscription set is still noticed within it.
    /// Human-readable form (`"30s"`).
    #[toml_comment(inline)]
    pub interval_max_sleep: HumanDuration,
}

impl Default for NotificationsConfig {
    fn default() -> NotificationsConfig {
        NotificationsConfig {
            delivery: DeliveryConfig::default(),
            streaming: NotificationConfig::default(),
            interval_max_sleep: HumanDuration::from_secs(30),
        }
    }
}

impl NotificationsConfig {
    /// Validate both notification sub-configurations and the interval ceiling.
    ///
    /// # Errors
    ///
    /// Returns [`NotificationsConfigError`] wrapping the delivery or streaming
    /// error its owner reported for whichever sub-configuration is invalid, or
    /// [`NotificationsConfigError::ZeroIntervalMaxSleep`] when the interval idle
    /// ceiling is zero.
    pub fn validate(&self) -> Result<(), NotificationsConfigError> {
        self.delivery.validate()?;
        self.streaming.validate()?;
        if self.interval_max_sleep.is_zero() {
            return Err(NotificationsConfigError::ZeroIntervalMaxSleep);
        }
        Ok(())
    }
}
