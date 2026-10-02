use crate::{
    change::{CsourceSubscriptionChange, UpsertKind},
    error::SubscriptionError,
    normalization,
    term_expansion,
    variant::{DatasetIdPolicy, SubscriptionVariant},
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_jsonld::{context_spec::ContextSpec, engine::processor::JsonLdEngine};
use andromeda_ngsi_ld_compaction::name_compaction::NameCompaction;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::subscription::Subscription;
use async_trait::async_trait;
use serde_json::Value;

/// CSR-subscription-specific behaviour.
pub struct CsrSubscriptionVariant {
    engine: JsonLdEngine,
}

impl CsrSubscriptionVariant {
    /// Construct context-source-subscription-specific lifecycle behavior.
    #[must_use]
    pub const fn new(engine: JsonLdEngine) -> CsrSubscriptionVariant {
        CsrSubscriptionVariant { engine }
    }
}

#[async_trait]
impl SubscriptionVariant for CsrSubscriptionVariant {
    type Change = CsourceSubscriptionChange;

    async fn expand_terms(&self, payload: &mut Value, context: &ContextSpec) -> Result<(), SubscriptionError> {
        term_expansion::expand_common_subscription_terms(&self.engine, payload, context).await
    }

    async fn serialize_for_response(&self, subscription: &Subscription, names: &NameCompaction) -> Result<Value, SubscriptionError> {
        let mut value = serde_json::to_value(subscription).map_err(SubscriptionError::Representation)?;
        normalization::fixup_expired_status(&mut value, subscription.lifecycle.expires_at);
        normalization::compact_subscription_iris(names, &mut value);
        normalization::compact_subscription_filters(names, &mut value, &subscription.filters).await;
        Ok(value)
    }

    fn validate_update_extra(&self, _payload: &Value) -> Result<(), SubscriptionError> {
        // CSR subscription updates (5.11.3) do NOT reject past expiresAt
        Ok(())
    }

    fn dataset_id_policy(&self) -> DatasetIdPolicy {
        DatasetIdPolicy::Discard
    }

    fn make_upserted(&self, target: ContextTarget, id: Identifier, kind: UpsertKind) -> CsourceSubscriptionChange {
        CsourceSubscriptionChange::Upserted { target, id, kind }
    }

    fn make_deleted(&self, target: ContextTarget, id: Identifier) -> CsourceSubscriptionChange {
        CsourceSubscriptionChange::Deleted { target, id }
    }
}
