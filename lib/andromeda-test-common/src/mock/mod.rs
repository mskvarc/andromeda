//! HTTP and MQTT mock servers used by notification and federation tests.
//!
//! - [`ContextSourceMock`] — axum stub for a Context Source, consumed by
//!   every `distop_*` suite (Batch 5).
//! - [`NotificationReceiver`] — axum stub that captures NGSI-LD
//!   notifications for subscription and CSR notification tests.
//! - [`MqttReceiver`] — MQTT client that captures messages published by the
//!   broker's MQTT endpoint.

pub mod context_source;
pub mod notification_http;
pub mod notification_mqtt;
