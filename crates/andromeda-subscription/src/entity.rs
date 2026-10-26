use crate::{
    change::{SubscriptionChange, UpsertKind},
    error::SubscriptionError,
    request_validation,
    term_expansion,
    variant::{DatasetIdPolicy, SubscriptionVariant},
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_jsonld::{context_spec::ContextSpec, engine::processor::JsonLdEngine};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::{notification::DEFAULT_NOTIFICATION_TRIGGERS, subscription::Subscription, subscription_request::SubscriptionRequest};
use async_trait::async_trait;

/// Entity-subscription-specific behaviour.
pub struct EntitySubscriptionVariant {
    engine: JsonLdEngine,
}

impl EntitySubscriptionVariant {
    /// Construct entity-subscription-specific lifecycle behavior.
    #[must_use]
    pub const fn new(engine: JsonLdEngine) -> EntitySubscriptionVariant {
        EntitySubscriptionVariant { engine }
    }
}

#[async_trait]
impl SubscriptionVariant for EntitySubscriptionVariant {
    type Change = SubscriptionChange;

    async fn expand_terms(&self, request: &mut SubscriptionRequest, context: &ContextSpec) -> Result<(), SubscriptionError> {
        term_expansion::expand_common_subscription_terms(&self.engine, request, context).await
    }

    /// Supply the clause 5.2.12 default `notificationTrigger` a Subscription
    /// that named none is matched under.
    ///
    /// Table 5.2.12-1: "If not present, the default is the combination
    /// attributeCreated and attributeUpdated". What the Context Subscriber wrote
    /// is what the broker stores, so the default is supplied when the
    /// Subscription is served rather than when it is created -- and it is
    /// supplied for the Subscription of clause 5.2.12, not for the Context
    /// Source Registration Subscription of clause 5.11, whose notifications
    /// carry a `triggerReason` (clause 5.3.2) that no notification trigger
    /// selects.
    fn present(&self, subscription: &mut Subscription) {
        subscription
            .selection
            .notification_trigger
            .get_or_insert_with(|| DEFAULT_NOTIFICATION_TRIGGERS.to_vec());
    }

    fn validate_members(&self, request: &SubscriptionRequest) -> Result<(), SubscriptionError> {
        request_validation::validate_no_temporal_q(request)
    }

    fn validate_update_extra(&self, request: &SubscriptionRequest) -> Result<(), SubscriptionError> {
        // Clause 5.8.2.4: an entity subscription update with a past `expiresAt` is rejected.
        request_validation::validate_expires_at_not_in_past(request)
    }

    fn dataset_id_policy(&self) -> DatasetIdPolicy {
        DatasetIdPolicy::Retain
    }

    fn make_upserted(&self, target: ContextTarget, id: Identifier, _kind: UpsertKind) -> SubscriptionChange {
        // An entity subscription's cache is rebuilt the same way whether the
        // subscription is new or replaced, so the change does not carry it.
        SubscriptionChange::Upserted { target, id }
    }

    fn make_deleted(&self, target: ContextTarget, id: Identifier) -> SubscriptionChange {
        SubscriptionChange::Deleted { target, id }
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{
        entity::EntitySubscriptionVariant,
        error::SubscriptionError,
        subscription_validation_reason::SubscriptionValidationReason,
        variant::SubscriptionVariant,
    };
    use andromeda_jsonld::engine::processor::JsonLdEngine;
    use andromeda_ngsi_ld_model::{
        notification::NotificationTrigger,
        request_body::RequestBody,
        subscription::Subscription,
        subscription_request::SubscriptionRequest,
    };
    use andromeda_ngsi_ld_representation::core_term_aliases::CoreTermAliases;
    use serde_json::json;

    fn request(wire: &serde_json::Value) -> SubscriptionRequest {
        SubscriptionRequest::from_request_text(wire.to_string().as_bytes(), CoreTermAliases::keyword_spellings()).expect("a readable request")
    }

    /// Table 5.2.12-1: `temporalQ` is "to be used only in Context Registration
    /// Subscriptions", so an entity subscription create or update refuses it.
    #[test]
    fn a_temporal_q_on_an_entity_subscription_is_refused() {
        let result = variant().validate_members(&request(&json!({
            "temporalQ": {"timerel": "after", "timeAt": "2020-06-01T00:00:00Z"}
        })));
        assert!(
            matches!(
                &result,
                Err(SubscriptionError::Validation(SubscriptionValidationReason::TemporalQNotForEntitySubscription))
            ),
            "got {result:?}"
        );
    }

    /// An update fragment's NGSI-LD Null removes the member, so a subscriber
    /// can still clear a `temporalQ` it holds.
    #[test]
    fn a_temporal_q_deletion_and_an_absent_temporal_q_are_admitted() {
        for wire in [json!({"temporalQ": "urn:ngsi-ld:null"}), json!({"isActive": true})] {
            let result = variant().validate_members(&request(&wire));
            assert!(result.is_ok(), "got {result:?}");
        }
    }

    fn subscription(members: &serde_json::Value) -> Subscription {
        let mut wire = json!({
            "id": "urn:ngsi-ld:Subscription:1",
            "type": "Subscription",
            "notification": {"endpoint": {"uri": "https://receiver.example.com/notify"}},
            "status": "active",
            "createdAt": "2024-01-01T00:00:00Z",
            "modifiedAt": "2024-01-01T00:00:00Z"
        });
        if let (Some(target), Some(extra)) = (wire.as_object_mut(), members.as_object()) {
            target.extend(extra.iter().map(|(key, value)| (key.clone(), value.clone()))); // clone: the fixture owns its members
        }
        serde_json::from_value(wire).expect("a valid subscription")
    }

    fn variant() -> EntitySubscriptionVariant {
        EntitySubscriptionVariant::new(JsonLdEngine::builder().allow_remote(false).build().expect("engine builds"))
    }

    /// Table 5.2.12-1: an entity subscription naming no `notificationTrigger`
    /// is served with the default of the combination attributeCreated and
    /// attributeUpdated.
    #[test]
    fn an_absent_notification_trigger_is_served_as_the_clause_default() {
        let mut served = subscription(&json!({}));
        variant().present(&mut served);
        pretty_assertions::assert_eq!(
            served.selection.notification_trigger,
            Some(vec![NotificationTrigger::AttributeCreated, NotificationTrigger::AttributeUpdated])
        );
    }

    /// What the subscriber wrote is what is served.
    #[test]
    fn a_written_notification_trigger_is_served_as_written() {
        let mut served = subscription(&json!({"notificationTrigger": ["entityDeleted"]}));
        variant().present(&mut served);
        pretty_assertions::assert_eq!(served.selection.notification_trigger, Some(vec![NotificationTrigger::EntityDeleted]));
    }
}
