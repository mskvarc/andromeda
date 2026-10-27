//! Sending one context source notification (ETSI GS CIM 009 clause 5.11.7).
//!
//! Four occasions send the same message -- a subscription that just started
//! matching, one that was just updated, a registration that changed, and a
//! `timeInterval` that elapsed -- and clause 5.11.7 describes one sending for
//! all of them: the structure of clause 5.3.2, the endpoint, then
//! `timesSent`, `lastNotification` and the success or failure stamp. Keeping
//! that in one type keeps the periodic sender and the change-driven one from
//! drifting apart, for instance on the `triggerReason` Table 5.3.2-1 makes
//! mandatory.

use crate::{error::NotificationError, notification_outcome::notification_outcome, resolved_notification_context::claimed_notification_context};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_delivery::{
    dispatch::TransportDispatcher,
    endpoint::transport::{EndpointTransport, parse_endpoint_transport},
    notification_body::NotificationBody,
    notification_context::NotificationContext,
    request::NotificationDelivery,
};
use andromeda_jsonld::{context_spec::ContextSpec, engine::processor::JsonLdEngine};
use andromeda_ngsi_ld_compaction::name_compaction::NameCompaction;
use andromeda_ngsi_ld_lexicon::{identifier::Identifier, ngsi_ld_urn::NotificationId};
use andromeda_ngsi_ld_model::{
    notification::{CsourceNotification, CsrTriggerReason},
    registration::ContextSourceRegistration,
};
use andromeda_registration::information_compaction::compacted_information;
use andromeda_subscription::{
    compiled::CompiledCsrSubscription,
    csource_matching::matching_information,
    notification_slot::{NotificationSlot, NotificationSlotGate},
    store::SubscriptionStore,
};
use andromeda_tenancy::tenant_ref::TenantRef;
use chrono::Utc;
use std::sync::Arc;
use tracing::{debug, error};

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
    /// Returns [`NotificationError::JsonLd`] when the subscription's context
    /// does not resolve or process (clause 5.5.6).
    pub(crate) async fn notified_registrations(
        &self,
        registrations: &[&ContextSourceRegistration],
        subscription: &CompiledCsrSubscription,
    ) -> Result<Vec<ContextSourceRegistration>, NotificationError> {
        let context = ContextSpec::for_subscription_context(subscription.jsonld_context.as_ref());
        let names = NameCompaction::new(self.engine.compactor(&context).processed_context().await?);
        Ok(registrations
            .iter()
            .map(|registration| notified_registration(&names, registration, subscription))
            .collect())
    }

    /// Deliver `data` -- the notified registrations -- to `subscription` with
    /// `trigger_reason`, honouring the subscription's `throttling` and its
    /// endpoint's `cooldown`, and recording the outcome on it (clause 5.11.7).
    ///
    /// The gate follows the subscription's cadence: a `timeInterval`
    /// subscription carries no `throttling` (Table 5.2.12-1 gives the member
    /// cardinality 0 when `timeInterval` is present) but records the instant
    /// it answers its grid at with the claim, and a change-driven one is
    /// gated by its `throttling` when set.
    pub(crate) async fn deliver(
        &self,
        subscription: &CompiledCsrSubscription,
        data: Vec<ContextSourceRegistration>,
        trigger_reason: CsrTriggerReason,
    ) -> CsourceDeliveryOutcome {
        let sub_id = &subscription.id;

        let record = match self
            .sub_repo
            .claim_notification_slot(
                &self.target,
                sub_id,
                NotificationSlotGate::at(Utc::now(), subscription.cadence(), subscription.endpoint_cooldown),
            )
            .await
        {
            Ok(NotificationSlot::Claimed(record)) => record,
            Ok(NotificationSlot::Suppressed) => return CsourceDeliveryOutcome::NotSent,
            Err(error) => {
                debug!("Failed to claim CSR notification slot for {}: {}", sub_id, error);
                return CsourceDeliveryOutcome::NotSent;
            }
        };

        let Some(endpoint_transport) = compiled_transport(subscription) else {
            return CsourceDeliveryOutcome::NotSent;
        };
        // Clause 5.5.6: a `jsonldContext` that cannot be loaded names no context
        // this notification can be rendered under.
        let Some(context) = claimed_notification_context(
            &self.engine,
            subscription.jsonld_context.as_ref(),
            self.sub_repo.as_ref(),
            &self.target,
            sub_id,
            record,
        )
        .await
        else {
            return CsourceDeliveryOutcome::NotSent;
        };
        let delivery = self.delivery_for(subscription, &context);
        let Ok(notification_id) = NotificationId::mint().map(Identifier::from) else {
            error!("Failed to construct notification URN for {}", sub_id);
            return CsourceDeliveryOutcome::NotSent;
        };
        let notification = CsourceNotification::builder()
            .context(delivery.body_context())
            .id(&notification_id)
            .subscription_id(sub_id)
            .notified_at(Utc::now())
            .data(&data)
            .trigger_reason(trigger_reason)
            .build();
        let body = match NotificationBody::written(&notification) {
            Ok(body) => body,
            Err(error) => {
                error!("Failed to write CsourceNotification for {}: {}", sub_id, error);
                return CsourceDeliveryOutcome::NotSent;
            }
        };
        let request = delivery.assemble(body);
        let delivery = self.transport.deliver(&endpoint_transport, &request).await;
        if let Err(error) = self
            .sub_repo
            .record_outcome(&self.target, sub_id, notification_outcome(&delivery), record)
            .await
        {
            debug!("Failed to record the CSR notification outcome for {}: {}", sub_id, error);
        }
        CsourceDeliveryOutcome::Sent
    }

    /// The delivery one CSR notification becomes.
    fn delivery_for<'a>(&'a self, subscription: &'a CompiledCsrSubscription, context: &'a NotificationContext) -> NotificationDelivery<'a> {
        NotificationDelivery::builder()
            .endpoint_uri(&subscription.endpoint_uri)
            .accept(subscription.endpoint_accept)
            .receiver_info(subscription.endpoint_receiver_info.as_deref())
            .context(context)
            .tenant(self.tenant.tenant_name())
            .timeout(subscription.endpoint_timeout)
            .subscription_id(&subscription.id)
            .build()
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
fn notified_registration(
    names: &NameCompaction,
    registration: &ContextSourceRegistration,
    subscription: &CompiledCsrSubscription,
) -> ContextSourceRegistration {
    let information = compacted_information(names, matching_information(subscription, registration));
    // clone: the notification carries its own trimmed copy of the stored registration
    let mut notified = registration.clone();
    notified.coverage.information = information;
    notified
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::csource_delivery::notified_registration;
    use andromeda_jsonld::{context_spec::ContextSpec, engine::processor::JsonLdEngine};
    use andromeda_ngsi_ld_compaction::name_compaction::NameCompaction;
    use andromeda_ngsi_ld_model::{
        registration::ContextSourceRegistration,
        subscription::{SubscriptionStatus, standing::SubscriptionStanding},
    };
    use andromeda_subscription::{compiled::CompiledCsrSubscription, geo_filter::SubscriptionGeoFilter};
    use serde_json::json;
    use url::Url;

    fn subscription(raw_type: &str) -> CompiledCsrSubscription {
        CompiledCsrSubscription {
            id: "urn:ngsi-ld:Subscription:csource".parse().expect("valid URI"),
            standing: SubscriptionStanding {
                status: SubscriptionStatus::Active,
                expires_at: None,
            },
            endpoint_uri: Url::parse("http://example.com/notify").expect("valid URL"),
            endpoint_accept: None,
            endpoint_receiver_info: None,
            endpoint_notifier_info: None,
            endpoint_cooldown: None,
            endpoint_timeout: None,
            selectors: vec![serde_json::from_value(json!({"type": raw_type})).expect("selector parses")],
            matching_attrs: Vec::new(),
            temporal_q: None,
            geo_q: SubscriptionGeoFilter::Unrestricted,
            scope_q: None,
            throttling_secs: None,
            time_interval_secs: None,
            jsonld_context: None,
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

    /// Names compacted under the default context, resolved offline.
    async fn core_names() -> NameCompaction {
        let engine = JsonLdEngine::builder().allow_remote(false).build().expect("the offline JSON-LD engine builds");
        NameCompaction::new(
            engine
                .compactor(&ContextSpec::none())
                .processed_context()
                .await
                .expect("the core context processes"),
        )
    }

    /// Clause 5.11.7: the notification carries only the matching
    /// `RegistrationInfo` elements.
    #[tokio::test]
    async fn the_notified_registration_keeps_only_the_matching_information() {
        let notified = notified_registration(&core_names().await, &registration(), &subscription("https://example.com/Vehicle"));
        let value = serde_json::to_value(notified).expect("serialises");

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

        let value = serde_json::to_value(notified_registration(&core_names().await, &registration, &subscription(vocabulary))).expect("serialises");

        pretty_assertions::assert_eq!(value["information"], json!([{"entities": [{"type": "Vehicle"}]}]));
    }
}
