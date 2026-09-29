//! Sending one context source notification (ETSI GS CIM 009 clause 5.11.7).
//!
//! Four occasions send the same message -- a subscription that just started
//! matching, one that was just updated, a registration that changed, and a
//! `timeInterval` that elapsed -- and clause 5.11.7 describes one sending for
//! all of them: the structure of clause 5.3.2, the endpoint, then
//! `timesSent`, `lastNotification` and the success or failure stamp. Keeping
//! that in one type is what stops the periodic sender and the change-driven
//! one from drifting apart, which is how the periodic one came to omit the
//! `triggerReason` Table 5.3.2-1 makes mandatory.

use crate::error::NotificationError;
use andromeda_context_space::context_target::ContextTarget;
use andromeda_delivery::{
    dispatch::TransportDispatcher,
    endpoint::transport::{EndpointTransport, parse_endpoint_transport},
    request::NotificationDelivery,
};
use andromeda_jsonld::{compaction::NameCompactor, context_spec::ContextSpec, engine::processor::JsonLdEngine};
use andromeda_ngsi_ld_lexicon::{identifier::Identifier, uri::NGSI_LD_URN_NID};
use andromeda_ngsi_ld_model::{
    notification::{CsourceNotification, CsourceNotificationType, CsrTriggerReason},
    registration::ContextSourceRegistration,
};
use andromeda_registration::information_compaction::compact_information;
use andromeda_subscription::{
    compiled::CompiledCsrSubscription,
    csource_matching::matching_information,
    notification_slot::{NotificationSlot, NotificationSlotGate},
    store::SubscriptionStore,
};
use andromeda_tenancy::tenant_ref::TenantRef;
use chrono::Utc;
use serde_json::Value;
use std::sync::Arc;
use tracing::{debug, error};
use uuid::Uuid;

/// Whether a notification reached its endpoint's transport.
///
/// Clause 5.11.7 counts a notification that failed at the endpoint as sent --
/// `timesSent` is incremented and only `lastFailure` tells it apart -- so the
/// subscriber may have received it either way. One held back before it was
/// sent, by the subscription's `throttling`, its endpoint's `cooldown` or a
/// payload that could not be built, never reached the subscriber.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CsourceDeliveryOutcome {
    /// The notification was handed to the endpoint's transport.
    Sent,
    /// The notification was held back before it was sent.
    NotSent,
}

/// The delivery half of one tenant's context source notification policy.
pub(crate) struct CsourceNotificationDelivery {
    /// The tenant branch the subscription's delivery state is recorded on.
    target: ContextTarget,
    /// The store the notification slot is claimed from and the outcome
    /// recorded in.
    sub_repo: Arc<dyn SubscriptionStore>,
    /// The processor compacting the notified registrations back through the
    /// subscription's @context.
    engine: JsonLdEngine,
    /// The dispatcher every transport binding is reached through.
    transport: Arc<TransportDispatcher>,
    /// The tenant name the delivery is addressed under.
    tenant: TenantRef,
}

impl CsourceNotificationDelivery {
    /// Construct the delivery half over the ports it addresses.
    pub(crate) const fn new(
        target: ContextTarget,
        sub_repo: Arc<dyn SubscriptionStore>,
        engine: JsonLdEngine,
        transport: Arc<TransportDispatcher>,
        tenant: TenantRef,
    ) -> CsourceNotificationDelivery {
        CsourceNotificationDelivery {
            target,
            sub_repo,
            engine,
            transport,
            tenant,
        }
    }

    /// The registrations as a notification for `subscription` carries them.
    ///
    /// Clause 5.11.7: implementations "should return filtered Context Source
    /// Registrations, which only contain context source registration
    /// information relevant for the subscription, in particular only matching
    /// `RegistrationInfo` elements", and the result is a notification body, so
    /// its entity types and attribute names are compacted back through the
    /// @context the notification is sent under rather than left in the
    /// expanded form the store holds.
    ///
    /// # Errors
    ///
    /// Returns [`NotificationError::Representation`] when a registration
    /// cannot be serialised.
    pub(crate) async fn notified_registrations(
        &self,
        registrations: &[&ContextSourceRegistration],
        subscription: &CompiledCsrSubscription,
    ) -> Result<Vec<Value>, NotificationError> {
        let context = ContextSpec::for_subscription_context(subscription.jsonld_context.as_ref());
        let names = NameCompactor::new(&self.engine, &context);
        let mut notified = Vec::with_capacity(registrations.len());
        for registration in registrations {
            notified.push(notified_registration(&names, registration, subscription).await?);
        }
        Ok(notified)
    }

    /// Deliver `data` -- the notified registrations -- to `subscription` with
    /// `trigger_reason`, honouring the subscription's `throttling` and its
    /// endpoint's `cooldown`, and recording the outcome on it (clause 5.11.7).
    ///
    /// A `timeInterval` subscription carries no `throttling` -- Table 5.2.12-1
    /// gives the member cardinality 0 when `timeInterval` is present -- so the
    /// one gate serves both cadences without either having to know which it is.
    pub(crate) async fn deliver(&self, subscription: &CompiledCsrSubscription, data: Vec<Value>, trigger_reason: CsrTriggerReason) -> CsourceDeliveryOutcome {
        let sub_id = &subscription.id;

        match self
            .sub_repo
            .claim_notification_slot(
                &self.target,
                sub_id,
                NotificationSlotGate::at(Utc::now(), subscription.throttling_secs, subscription.endpoint_cooldown),
            )
            .await
        {
            Ok(NotificationSlot::Claimed) => {}
            Ok(NotificationSlot::Suppressed) => return CsourceDeliveryOutcome::NotSent,
            Err(error) => {
                debug!("Failed to claim CSR notification slot for {}: {}", sub_id, error);
                return CsourceDeliveryOutcome::NotSent;
            }
        }

        let Ok(notification_id) = format!("urn:{}:Notification:{}", NGSI_LD_URN_NID, Uuid::new_v4()).parse::<Identifier>() else {
            error!("Failed to construct notification URN for {}", sub_id);
            return CsourceDeliveryOutcome::NotSent;
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
            Ok(payload) => payload,
            Err(error) => {
                error!("Failed to serialize CsourceNotification for {}: {}", sub_id, error);
                return CsourceDeliveryOutcome::NotSent;
            }
        };

        let Some(endpoint_transport) = compiled_transport(subscription) else {
            return CsourceDeliveryOutcome::NotSent;
        };
        let request = self.request_for(subscription, &payload).assemble();
        let success = self.transport.deliver(&endpoint_transport, &request).await.is_delivered();

        if success {
            if let Err(error) = self.sub_repo.mark_success(&self.target, sub_id).await {
                debug!("Failed to mark CSR notification success for {}: {}", sub_id, error);
            }
        } else if let Err(error) = self.sub_repo.mark_failure(&self.target, sub_id).await {
            debug!("Failed to mark CSR notification failure for {}: {}", sub_id, error);
        }
        CsourceDeliveryOutcome::Sent
    }

    /// The delivery one CSR notification becomes.
    fn request_for<'a>(&'a self, subscription: &'a CompiledCsrSubscription, payload: &'a Value) -> NotificationDelivery<'a> {
        NotificationDelivery {
            payload,
            endpoint_uri: &subscription.endpoint_uri,
            accept: subscription.endpoint_accept.as_ref(),
            receiver_info: subscription.endpoint_receiver_info.as_deref(),
            jsonld_context: subscription.jsonld_context.as_ref(),
            tenant: self.tenant.tenant_name(),
            snapshot: None,
            timeout: subscription.endpoint_timeout,
            subscription_id: &subscription.id,
        }
    }
}

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

/// One registration as a notification carries it: trimmed to the
/// `RegistrationInfo` elements `subscription` matches (clause 5.11.7) and
/// compacted under the context `names` compacts against.
async fn notified_registration(
    names: &NameCompactor<'_>,
    registration: &ContextSourceRegistration,
    subscription: &CompiledCsrSubscription,
) -> Result<Value, NotificationError> {
    let mut value = serde_json::to_value(registration).map_err(|error| NotificationError::representation(error.to_string()))?;
    let matching = matching_information(subscription, registration)
        .into_iter()
        .map(serde_json::to_value)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| NotificationError::representation(error.to_string()))?;
    if let Some(object) = value.as_object_mut() {
        object.insert("information".to_string(), Value::Array(matching));
    }
    if let Some(information) = value.get_mut("information") {
        compact_information(names, information).await;
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use crate::csource_delivery::notified_registration;
    use andromeda_jsonld::{compaction::NameCompactor, context_spec::ContextSpec, engine::processor::JsonLdEngine};
    use andromeda_ngsi_ld_model::{registration::ContextSourceRegistration, subscription::SubscriptionStatus};
    use andromeda_subscription::compiled::CompiledCsrSubscription;
    use serde_json::json;
    use url::Url;

    fn subscription(raw_type: &str) -> CompiledCsrSubscription {
        CompiledCsrSubscription {
            id: "urn:ngsi-ld:Subscription:csource".parse().expect("valid URI"),
            status: SubscriptionStatus::Active,
            endpoint_uri: Url::parse("http://example.com/notify").expect("valid URL"),
            endpoint_accept: None,
            endpoint_receiver_info: None,
            endpoint_notifier_info: None,
            endpoint_cooldown: None,
            endpoint_timeout: None,
            selectors: vec![serde_json::from_value(json!({"type": raw_type})).expect("selector parses")],
            matching_attrs: Vec::new(),
            temporal_q: None,
            geo_q: None,
            scope_q: None,
            throttling_secs: None,
            time_interval_secs: None,
            jsonld_context: None,
            expires_at: None,
            compiled_csf: None,
        }
    }

    fn registration() -> ContextSourceRegistration {
        serde_json::from_value(json!({
            "id": "urn:ngsi-ld:ContextSourceRegistration:1",
            "type": "ContextSourceRegistration",
            "information": [
                {"entities": [{"type": "https://example.com/Vehicle"}]},
                {"entities": [{"type": "https://example.com/Building"}]}
            ],
            "endpoint": "https://source.example.com/",
            "status": "ok",
            "createdAt": "2024-01-01T00:00:00Z",
            "modifiedAt": "2024-01-02T00:00:00Z"
        }))
        .expect("registration parses")
    }

    fn engine() -> JsonLdEngine {
        JsonLdEngine::builder().allow_remote(false).build().expect("the offline JSON-LD engine builds")
    }

    /// Clause 5.11.7: the notification carries only the matching
    /// `RegistrationInfo` elements.
    #[tokio::test]
    async fn the_notified_registration_keeps_only_the_matching_information() {
        let value = notified_registration(
            &NameCompactor::new(&engine(), &ContextSpec::none()),
            &registration(),
            &subscription("https://example.com/Vehicle"),
        )
        .await
        .expect("serialises");

        pretty_assertions::assert_eq!(value["information"], json!([{"entities": [{"type": "https://example.com/Vehicle"}]}]));
        pretty_assertions::assert_eq!(value["id"], json!("urn:ngsi-ld:ContextSourceRegistration:1"));
    }

    /// Clause 5.11.7 sends a notification body, so the registration reaches
    /// the subscriber under the @context the notification is sent with rather
    /// than in the expanded form the store holds.
    #[tokio::test]
    async fn the_notified_registration_is_compacted_under_the_notification_context() {
        let vocabulary = "https://uri.etsi.org/ngsi-ld/default-context/Vehicle";
        let registration: ContextSourceRegistration = serde_json::from_value(json!({
            "id": "urn:ngsi-ld:ContextSourceRegistration:2",
            "type": "ContextSourceRegistration",
            "information": [{"entities": [{"type": vocabulary}]}],
            "endpoint": "https://source.example.com/",
            "createdAt": "2024-01-01T00:00:00Z",
            "modifiedAt": "2024-01-02T00:00:00Z"
        }))
        .expect("registration parses");

        let value = notified_registration(&NameCompactor::new(&engine(), &ContextSpec::none()), &registration, &subscription(vocabulary))
            .await
            .expect("serialises");

        pretty_assertions::assert_eq!(value["information"], json!([{"entities": [{"type": "Vehicle"}]}]));
    }
}
