//! HTTP and MQTT mock servers used by notification and federation tests.
//!
//! - [`ContextSourceMock`] — axum stub for a Context Source, consumed by
//!   every `distop_*` suite (Batch 5).
//! - [`NotificationReceiver`] — axum stub that captures NGSI-LD
//!   notifications for subscription and CSR notification tests.
//! - [`MqttReceiver`] — MQTT client that captures messages published by the
//!   broker's MQTT endpoint.

mod context_source;
mod notification_http;
mod notification_mqtt;

pub use context_source::ContextSourceMock;
pub use notification_http::{CapturedNotification, NotificationReceiver};
pub use notification_mqtt::{MqttMessage, MqttReceiver};
