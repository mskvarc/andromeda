//! Delivery binding for snapshot status notifications.

use andromeda_delivery::{dispatch::TransportDispatcher, endpoint::transport::EndpointTransport, request::NotificationDelivery};
use andromeda_ngsi_ld_model::snapshot::Snapshot;
use andromeda_snapshot::notification::SnapshotNotificationSink;
use futures::future::BoxFuture;
use std::sync::Arc;

/// Delivers snapshot status notifications through the configured transport dispatcher.
pub struct DeliverySnapshotNotificationSink {
    transport: Arc<TransportDispatcher>,
}

impl DeliverySnapshotNotificationSink {
    /// Construct the notification binding.
    pub const fn new(transport: Arc<TransportDispatcher>) -> DeliverySnapshotNotificationSink {
        DeliverySnapshotNotificationSink { transport }
    }
}

impl SnapshotNotificationSink for DeliverySnapshotNotificationSink {
    fn deliver<'a>(&'a self, snapshot: &'a Snapshot) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            let Some(endpoint) = snapshot.endpoint.as_ref() else {
                return;
            };
            let notification = snapshot.build_notification();
            let payload = match serde_json::to_value(notification) {
                Ok(payload) => payload,
                Err(error) => {
                    tracing::error!(%error, snapshot_id = %snapshot.id, "snapshot notification serialization failed");
                    return;
                }
            };
            let request = NotificationDelivery {
                payload: &payload,
                endpoint_uri: endpoint,
                accept: None,
                receiver_info: snapshot.receiver_info.as_deref(),
                jsonld_context: None,
                tenant: None,
                subscription_id: &snapshot.id,
            }
            .assemble();
            if !self.transport.deliver(&EndpointTransport::Http, &request).await.is_delivered() {
                tracing::warn!(snapshot_id = %snapshot.id, "snapshot notification delivery failed");
            }
        })
    }
}
