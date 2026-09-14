//! Composition of the durable-delivery and streaming notification configurations.

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
}

/// Durable-delivery and streaming notification configuration.
///
/// The two sub-configurations are unrelated in what they own -- one governs the
/// durable HTTP/MQTT delivery queue, the other the SSE and WebSocket streaming
/// transports -- but both are one operator-facing `notifications` section.
#[derive(Deserialize, Serialize, Default, TomlComment, Debug, Clone)]
pub struct NotificationsConfig {
    /// Durable notification event and delivery configuration.
    pub delivery: DeliveryConfig,
    /// Streaming (Server-Sent Events and WebSocket) transport configuration.
    pub streaming: NotificationConfig,
}

impl NotificationsConfig {
    /// Validate both notification sub-configurations.
    ///
    /// # Errors
    ///
    /// Returns [`NotificationsConfigError`] wrapping the delivery or streaming
    /// error its owner reported for whichever sub-configuration is invalid.
    pub fn validate(&self) -> Result<(), NotificationsConfigError> {
        self.delivery.validate()?;
        self.streaming.validate()?;
        Ok(())
    }
}
