//! Delivery of one context source notification (clause 5.11.7).
//!
//! Three occasions send the same notification -- a subscription that just
//! started matching, a registration that changed, and a registration that
//! stopped matching -- and every one of them claims the subscription's
//! notification slot, builds a clause 5.3.2 `CsourceNotification`, addresses
//! the endpoint identically and records the outcome the same way.

use crate::csource_processor::CsourceNotificationProcessor;
use andromeda_delivery::{
    endpoint::transport::{EndpointTransport, parse_endpoint_transport},
    request::NotificationDelivery,
};
use andromeda_ngsi_ld_lexicon::uri::{NGSI_LD_URN_NID, Uri};
use andromeda_ngsi_ld_model::notification::{CsourceNotification, CsourceNotificationType, CsrTriggerReason};
use andromeda_subscription::{compiled::CompiledCsrSubscription, throttle::ceiling_for_throttle};
use chrono::Utc;
use serde_json::Value;
use tracing::{debug, error};
use uuid::Uuid;

/// The transport a compiled subscription's endpoint is reached over, or
/// `None` when the endpoint is no longer deliverable.
fn compiled_transport(subscription: &CompiledCsrSubscription) -> Option<EndpointTransport> {
    match parse_endpoint_transport(&subscription.endpoint_uri, subscription.endpoint_notifier_info.as_deref()) {
        Ok(transport) => Some(transport),
        Err(error) => {
            debug!(subscription = %subscription.id, %error, "Compiled context-source subscription endpoint is no longer deliverable");
            None
        }
    }
}

impl CsourceNotificationProcessor {
    /// The delivery one CSR notification becomes.
    fn delivery_for<'a>(&'a self, compiled_sub: &'a CompiledCsrSubscription, payload: &'a Value) -> NotificationDelivery<'a> {
        NotificationDelivery {
            payload,
            endpoint_uri: &compiled_sub.endpoint_uri,
            accept: compiled_sub.endpoint_accept.as_ref(),
            receiver_info: compiled_sub.endpoint_receiver_info.as_deref(),
            jsonld_context: compiled_sub.jsonld_context.as_ref(),
            tenant: self.tenant.tenant_name(),
            snapshot: None,
            subscription_id: &compiled_sub.id,
        }
    }

    /// Deliver `data` -- the notified registrations -- to `compiled_sub` with
    /// `trigger_reason`, honouring the subscription's throttling and recording
    /// the outcome on it (clause 5.11.7).
    pub(super) async fn deliver_csr_notification(&self, compiled_sub: &CompiledCsrSubscription, data: Vec<Value>, trigger_reason: CsrTriggerReason) {
        let sub_id = &compiled_sub.id;

        match self
            .sub_repo
            .claim_notification_slot(&self.target, sub_id, ceiling_for_throttle(Utc::now(), compiled_sub.throttling_secs))
            .await
        {
            Ok(true) => {}
            Ok(false) => return,
            Err(e) => {
                debug!("Failed to claim CSR notification slot for {}: {}", sub_id, e);
                return;
            }
        }

        let notification_id = if let Ok(u) = format!("urn:{}:Notification:{}", NGSI_LD_URN_NID, Uuid::new_v4()).parse::<Uri>() { u } else {
            error!("Failed to construct notification URN for {}", sub_id);
            return;
        };
        let notification = CsourceNotification::builder()
            .id(notification_id)
            .type_(CsourceNotificationType)
            .subscription_id(sub_id.clone()) // clone: the notification owns its subscription id and the subscription is still needed for delivery
            .notified_at(Utc::now())
            .data(data)
            .trigger_reason(Some(trigger_reason))
            .build();

        let payload = match serde_json::to_value(&notification) {
            Ok(v) => v,
            Err(e) => {
                error!("Failed to serialize CsourceNotification for {}: {}", sub_id, e);
                return;
            }
        };

        let Some(endpoint_transport) = compiled_transport(compiled_sub) else {
            return;
        };
        let request = self.delivery_for(compiled_sub, &payload).assemble();
        let success = self.transport.deliver(&endpoint_transport, &request).await.is_delivered();

        if success {
            if let Err(e) = self.sub_repo.mark_success(&self.target, sub_id).await {
                debug!("Failed to mark CSR notification success for {}: {}", sub_id, e);
            }
        } else if let Err(e) = self.sub_repo.mark_failure(&self.target, sub_id).await {
            debug!("Failed to mark CSR notification failure for {}: {}", sub_id, e);
        }
    }
}
