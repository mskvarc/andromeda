use crate::{
    change::{SubscriptionChange, UpsertKind},
    error::SubscriptionError,
    normalization,
    term_expansion,
    validation,
    variant::{DatasetIdPolicy, SubscriptionVariant},
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_jsonld::{compaction::NameCompactor, context_spec::ContextSpec, engine::processor::JsonLdEngine};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::subscription::Subscription;
use async_trait::async_trait;
use serde_json::Value;

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

    async fn expand_terms(&self, payload: &mut Value, context: &ContextSpec) -> Result<(), SubscriptionError> {
        term_expansion::expand_common_subscription_terms(&self.engine, payload, context).await
    }

    async fn serialize_for_response(&self, subscription: &Subscription, names: &NameCompactor<'_>) -> Result<Value, SubscriptionError> {
        let mut value = serde_json::to_value(subscription).map_err(SubscriptionError::Representation)?;

        normalization::fixup_expired_status(&mut value, subscription.lifecycle.expires_at);
        normalization::compact_subscription_iris(names, &mut value).await;
        normalization::compact_subscription_filters(names, &mut value, &subscription.filters).await;
        normalization::fixup_integer_fields(&mut value);
        normalization::fixup_default_notification_trigger(&mut value);

        Ok(value)
    }

    fn validate_update_extra(&self, payload: &Value) -> Result<(), SubscriptionError> {
        // Per spec 5.8.2.4: past expiresAt must be rejected for entity subscription updates
        validation::validate_expires_at_not_in_past(payload)
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
