//! In-process event bus: the change events the service and worker layers emit
//! and consume (ETSI GS CIM 009 clause 5.3).
//!
//! These carry no serde — they never cross the wire. The notification payloads
//! they produce ([`andromeda_ngsi_ld_model::notification::Notification`],
//! [`andromeda_ngsi_ld_model::notification::CsourceNotification`]) are the
//! serialisable shapes and stay in the model crate.

pub mod registration_change_sink;
pub mod subscription_change_sink;
pub mod subscription_endpoint_validator;
