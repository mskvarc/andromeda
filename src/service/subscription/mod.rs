/// The subscription store port, and the field sets it takes.
pub mod store;

use crate::{
    error::AndromedaError,
    event::subscription_change::SubscriptionChange,
    service::{
        subscription_base::{SubscriptionServiceCore, SubscriptionVariant},
        subscription_core,
    },
};
use andromeda_jsonld::{context_spec::ContextSpec, engine::processor::JsonLdEngine};
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_model::subscription::Subscription;
use andromeda_postgresql_context_store::{jsonld_context::repo::JsonLdContextRepository, subscription::repo::SubscriptionRepository};
use async_trait::async_trait;
use serde_json::Value;
use sqlx::postgres::PgPool;
use tokio::sync::broadcast;
use tracing::{debug, warn};
use url::Url;

/// Entity-subscription-specific behaviour.
pub struct EntitySubscriptionVariant {
    engine: JsonLdEngine,
    context_repo: JsonLdContextRepository,
}

#[async_trait]
impl SubscriptionVariant for EntitySubscriptionVariant {
    type Change = SubscriptionChange;

    async fn expand_terms(&self, payload: &mut Value, context: &ContextSpec, base_url: Option<&Url>) -> Result<(), AndromedaError> {
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
                    match self.context_repo.create_implicitly(hosted_payload, base_url).await {
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
                _other => None,
            };

            if let Some(stored) = stored
                && let Some(obj) = payload.as_object_mut()
            {
                obj.insert("jsonldContext".to_string(), stored);
            }
        }

        subscription_core::validation::expand_common_subscription_terms(&self.engine, payload, context).await
    }

    async fn serialize_for_response(&self, subscription: &Subscription, context: &ContextSpec) -> Result<Value, AndromedaError> {
        let mut value = serde_json::to_value(subscription).map_err(|e| AndromedaError::subscription_validation(format!("Serialization failed: {}", e)))?;

        subscription_core::normalization::compact_subscription_iris(&self.engine, &mut value, context).await;
        subscription_core::normalization::fixup_integer_fields(&mut value);
        subscription_core::normalization::fixup_default_notification_trigger(&mut value);
        subscription_core::normalization::fixup_jsonld_context(&mut value);

        Ok(value)
    }

    fn validate_update_extra(&self, payload: &Value) -> Result<(), AndromedaError> {
        // Per spec 5.8.2.4: past expiresAt must be rejected for entity subscription updates
        subscription_core::validation::validate_expires_at_not_in_past(payload)
    }

    fn use_dataset_id(&self) -> bool {
        true
    }

    fn use_entity_sub_merge_mode(&self) -> bool {
        true
    }

    fn make_upserted(&self, id: Uri) -> SubscriptionChange {
        SubscriptionChange::Upserted(id)
    }

    fn make_deleted(&self, id: Uri) -> SubscriptionChange {
        SubscriptionChange::Deleted(id)
    }
}

impl SubscriptionServiceCore<SubscriptionRepository, EntitySubscriptionVariant> {
    /// Create a new entity subscription service backed by the given database pool.
    pub fn new(
        pool: &PgPool,
        engine: JsonLdEngine,
        sub_change_tx: Option<broadcast::Sender<SubscriptionChange>>,
    ) -> SubscriptionServiceCore<SubscriptionRepository, EntitySubscriptionVariant> {
        let repo = SubscriptionRepository::new(pool);
        let context_repo = JsonLdContextRepository::new(pool);
        debug!("Subscription service created!");
        SubscriptionServiceCore {
            repo,
            engine: engine.clone(), // clone: engine is shared between service and variant
            variant: EntitySubscriptionVariant { engine, context_repo },
            change_tx: sub_change_tx,
        }
    }
}
