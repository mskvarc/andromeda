use crate::{
    change::CsourceSubscriptionChange,
    error::SubscriptionError,
    lifecycle::{DatasetIdPolicy, SubscriptionVariant},
    normalization,
    validation,
};
use andromeda_jsonld::{context_spec::ContextSpec, engine::processor::JsonLdEngine};
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
    pub const fn new(engine: JsonLdEngine) -> CsrSubscriptionVariant {
        CsrSubscriptionVariant { engine }
    }
}

#[async_trait]
impl SubscriptionVariant for CsrSubscriptionVariant {
    type Change = CsourceSubscriptionChange;

    async fn expand_terms(&self, payload: &mut Value, context: &ContextSpec, _base_url: Option<&Url>) -> Result<(), SubscriptionError> {
        // Store body @context as jsonldContext (simplified -- no context hosting).
        // The body context comes from ContextSpec (set by the JsonLd extractor), not
        // from the payload directly, since the extractor strips @context before
        // deserialisation.
        if payload.get("jsonldContext").is_none()
            && let Some(ctx_val) = context.body_context()
        {
            let jsonld_context_value = match ctx_val {
                Value::String(s) => Value::String(s.clone()), // clone: need owned String for new Value; original is borrowed from ContextSpec
                Value::Array(_) | Value::Object(_) => ctx_val.clone(), // clone: need owned copy to insert into payload as jsonldContext
                _other @ (Value::Null | Value::Bool(_) | Value::Number(_)) => Value::Null,
            };

            if jsonld_context_value != Value::Null
                && let Some(obj) = payload.as_object_mut()
            {
                let stored = match jsonld_context_value {
                    Value::String(s) => Value::String(s),
                    other @ (Value::Null | Value::Bool(_) | Value::Number(_) | Value::Array(_) | Value::Object(_)) => Value::String(other.to_string()),
                };
                obj.insert("jsonldContext".to_string(), stored);
            }
        }

        validation::expand_common_subscription_terms(&self.engine, payload, context).await
    }

    async fn serialize_for_response(&self, subscription: &Subscription, context: &ContextSpec) -> Result<Value, SubscriptionError> {
        let mut value = serde_json::to_value(subscription).map_err(|e| SubscriptionError::validation(format!("Serialization failed: {}", e)))?;
        normalization::fixup_expired_status(&mut value, subscription.lifecycle.expires_at);
        normalization::compact_subscription_iris(&self.engine, &mut value, context).await;
        Ok(value)
    }

    fn validate_update_extra(&self, _payload: &Value) -> Result<(), SubscriptionError> {
        // CSR subscription updates (5.11.3) do NOT reject past expiresAt
        Ok(())
    }

    fn dataset_id_policy(&self) -> DatasetIdPolicy {
        DatasetIdPolicy::Discard
    }

    fn make_upserted(&self, id: Uri) -> CsourceSubscriptionChange {
        CsourceSubscriptionChange::Upserted(id)
    }

    fn make_deleted(&self, id: Uri) -> CsourceSubscriptionChange {
        CsourceSubscriptionChange::Deleted(id)
    }
}
