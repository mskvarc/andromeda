//! In-process event bus: the change events the service and task layers emit
//! and consume (ETSI GS CIM 009 clause 5.3).
//!
//! These carry no serde — they never cross the wire. The notification payloads
//! they produce ([`andromeda_ngsi_ld_model::notification::Notification`],
//! [`andromeda_ngsi_ld_model::notification::CsourceNotification`]) are the
//! serialisable shapes and stay in the model crate.

pub(crate) mod entity_change_sink;
pub(crate) mod queued_handoff;
pub(crate) mod registration_change_sink;
pub(crate) mod snapshot_notification_sink;
pub(crate) mod snapshot_status_queue;
pub(crate) mod subscription_change_sink;
pub(crate) mod subscription_endpoint_validator;
