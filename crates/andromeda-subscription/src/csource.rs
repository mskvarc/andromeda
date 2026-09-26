use crate::{
    change::CsourceSubscriptionChange,
    error::SubscriptionError,
    normalization,
    notification_context,
    term_expansion,
    variant::{DatasetIdPolicy, SubscriptionVariant},
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_jsonld::{compaction::NameCompactor, context_spec::ContextSpec, engine::processor::JsonLdEngine};
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_model::subscription::Subscription;
use async_trait::async_trait;
use serde_json::Value;
use url::Url;

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

    async fn expand_terms(&self, payload: &mut Value, context: &ContextSpec, base_url: Option<&Url>) -> Result<(), SubscriptionError> {
        // Clause 5.8.2.4: initialise `jsonldContext` from the `@context`
        // applicable at creation when the request named none itself. A Context
        // Source Registration Subscription hosts no compound Context (`None`
        // host), so a request naming several contexts, or one inline, that
        // cannot reduce to a single URI falls to the core context (clause 5.5.5)
        // rather than storing an undereferenceable value.
        notification_context::initialize_notification_context(payload, context, base_url, None).await;

        term_expansion::expand_common_subscription_terms(&self.engine, payload, context).await
    }

    async fn serialize_for_response(&self, subscription: &Subscription, names: &NameCompactor<'_>) -> Result<Value, SubscriptionError> {
        let mut value =
            serde_json::to_value(subscription).map_err(|e| SubscriptionError::internal(format!("serializing subscription for response failed: {e}")))?;
        normalization::fixup_expired_status(&mut value, subscription.lifecycle.expires_at);
        normalization::compact_subscription_iris(names, &mut value).await;
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

    fn make_upserted(&self, target: ContextTarget, id: Uri) -> CsourceSubscriptionChange {
        CsourceSubscriptionChange::Upserted { target, id }
    }

    fn make_deleted(&self, target: ContextTarget, id: Uri) -> CsourceSubscriptionChange {
        CsourceSubscriptionChange::Deleted { target, id }
    }
}
