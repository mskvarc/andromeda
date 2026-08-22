use crate::{
    change::SubscriptionChange,
    error::SubscriptionError,
    lifecycle::{DatasetIdPolicy, SubscriptionVariant},
    normalization,
    validation,
};
use andromeda_context_registry::service::ContextRegistryService;
use andromeda_jsonld::{context_spec::ContextSpec, engine::processor::JsonLdEngine};
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_model::subscription::Subscription;
use async_trait::async_trait;
use serde_json::Value;
use std::sync::Arc;
use tracing::warn;
use url::Url;

/// Entity-subscription-specific behaviour.
pub struct EntitySubscriptionVariant {
    engine: JsonLdEngine,
    contexts: Arc<ContextRegistryService>,
}

impl EntitySubscriptionVariant {
    /// Construct entity-subscription-specific lifecycle behavior.
    pub const fn new(engine: JsonLdEngine, contexts: Arc<ContextRegistryService>) -> EntitySubscriptionVariant {
        EntitySubscriptionVariant { engine, contexts }
    }
}

#[async_trait]
impl SubscriptionVariant for EntitySubscriptionVariant {
    type Change = SubscriptionChange;

    async fn expand_terms(&self, payload: &mut Value, context: &ContextSpec, base_url: Option<&Url>) -> Result<(), SubscriptionError> {
        // Preserve body @context as jsonldContext — entity subs host compound contexts.
        // The body context comes from ContextSpec (set by the JsonLd extractor), not
        // from the payload directly, since the extractor strips @context before
        // deserialisation.
        if payload.get("jsonldContext").is_none()
            && let Some(ctx_val) = context.body_context()
        {
            let stored = match ctx_val {
                Value::String(s) => Some(Value::String(s.clone())), // clone: need owned String for new Value; s is borrowed from ctx_val
                Value::Array(arr) if arr.len() == 1 && arr[0].is_string() => Some(arr[0].clone()), // clone: extracting single element from array as owned Value
                Value::Array(_) | Value::Object(_) => {
                    let hosted_payload = serde_json::json!({ "@context": ctx_val });
                    match self.contexts.create_implicitly(hosted_payload, base_url).await {
                        Ok(urn) => {
                            let stored_value = match base_url {
                                Some(base) => {
                                    let mut hosted = base.clone(); // clone: the request origin remains reusable.
                                    hosted.set_path(&format!("/ngsi-ld/v1/jsonldContexts/{urn}"));
                                    hosted.to_string()
                                }
                                None => urn.to_string(),
                            };
                            Some(Value::String(stored_value))
                        }
                        Err(e) => {
                            warn!("Failed to create hosted context for subscription @context: {}", e);
                            None
                        }
                    }
                }
                _other @ (Value::Null | Value::Bool(_) | Value::Number(_)) => None,
            };

            if let Some(stored) = stored
                && let Some(obj) = payload.as_object_mut()
            {
                obj.insert("jsonldContext".to_string(), stored);
            }
        }

        validation::expand_common_subscription_terms(&self.engine, payload, context).await
    }

    async fn serialize_for_response(&self, subscription: &Subscription, context: &ContextSpec) -> Result<Value, SubscriptionError> {
        let mut value = serde_json::to_value(subscription).map_err(|e| SubscriptionError::validation(format!("Serialization failed: {}", e)))?;

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

    fn make_upserted(&self, id: Uri) -> SubscriptionChange {
        SubscriptionChange::Upserted(id)
    }

    fn make_deleted(&self, id: Uri) -> SubscriptionChange {
        SubscriptionChange::Deleted(id)
    }
}
