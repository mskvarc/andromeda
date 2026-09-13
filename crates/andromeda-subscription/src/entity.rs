use crate::{
    change::SubscriptionChange,
    error::SubscriptionError,
    normalization,
    notification_context,
    term_expansion,
    validation,
    variant::{DatasetIdPolicy, SubscriptionVariant},
};
use andromeda_context_registry::service::ContextRegistryService;
use andromeda_context_space::context_target::ContextTarget;
use andromeda_jsonld::{context_spec::ContextSpec, engine::processor::JsonLdEngine};
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_model::subscription::Subscription;
use async_trait::async_trait;
use serde_json::Value;
use std::sync::Arc;
use url::Url;

/// Entity-subscription-specific behaviour.
pub struct EntitySubscriptionVariant {
    engine: JsonLdEngine,
    contexts: Arc<ContextRegistryService>,
}

impl EntitySubscriptionVariant {
    /// Construct entity-subscription-specific lifecycle behavior.
    #[must_use]
    pub const fn new(engine: JsonLdEngine, contexts: Arc<ContextRegistryService>) -> EntitySubscriptionVariant {
        EntitySubscriptionVariant { engine, contexts }
    }
}

#[async_trait]
impl SubscriptionVariant for EntitySubscriptionVariant {
    type Change = SubscriptionChange;

    async fn expand_terms(&self, payload: &mut Value, context: &ContextSpec, base_url: Option<&Url>) -> Result<(), SubscriptionError> {
        // Clause 5.8.2.4: initialise `jsonldContext` from the `@context`
        // applicable at creation when the request named none itself, hosting a
        // compound applicable context as a broker-served Context so it reduces
        // to the single URI the field holds. The applicable contexts come from
        // the request's `ContextSpec` (the extractor strips `@context` from the
        // body before deserialisation), which carries both the `Link` header
        // contexts and the body context.
        notification_context::initialize_notification_context(payload, context, base_url, Some(&self.contexts)).await;

        term_expansion::expand_common_subscription_terms(&self.engine, payload, context).await
    }

    async fn serialize_for_response(&self, subscription: &Subscription, context: &ContextSpec) -> Result<Value, SubscriptionError> {
        let mut value = serde_json::to_value(subscription).map_err(|e| SubscriptionError::validation(format!("Serialization failed: {e}")))?;

        normalization::fixup_expired_status(&mut value, subscription.lifecycle.expires_at);
        normalization::compact_subscription_iris(&self.engine, &mut value, context).await;
        normalization::compact_subscription_filters(&self.engine, &mut value, &subscription.filters, context).await;
        normalization::fixup_integer_fields(&mut value);
        normalization::fixup_default_notification_trigger(&mut value);
        normalization::fixup_jsonld_context(&mut value);

        Ok(value)
    }

    fn validate_update_extra(&self, payload: &Value) -> Result<(), SubscriptionError> {
        // Per spec 5.8.2.4: past expiresAt must be rejected for entity subscription updates
        validation::validate_expires_at_not_in_past(payload)
    }

    fn dataset_id_policy(&self) -> DatasetIdPolicy {
        DatasetIdPolicy::Retain
    }

    fn make_upserted(&self, target: ContextTarget, id: Uri) -> SubscriptionChange {
        SubscriptionChange::Upserted { target, id }
    }

    fn make_deleted(&self, target: ContextTarget, id: Uri) -> SubscriptionChange {
        SubscriptionChange::Deleted { target, id }
    }
}
