use crate::{
    core::error::AndromedaError,
    jsonld::{ContextSpec, JsonLdEngine},
    repository::{jsonld_context::repo::JsonLdContextRepository, subscription::repo::SubscriptionRepository},
    service::{
        subscription_base::{SubscriptionServiceCore, SubscriptionVariant},
        subscription_core,
    },
};
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_model::{notification::SubscriptionChange, subscription::Subscription};
use async_trait::async_trait;
use serde_json::Value;
use sqlx::postgres::PgPool;
use tokio::sync::broadcast;
use tracing::{debug, warn};

/// Entity-subscription-specific behaviour.
pub struct EntitySubscriptionVariant {
    engine: JsonLdEngine,
    context_repo: JsonLdContextRepository,
}

#[async_trait]
impl SubscriptionVariant for EntitySubscriptionVariant {
    type Change = SubscriptionChange;

    async fn expand_terms(&self, payload: &mut Value, context: &ContextSpec, base_url: Option<&str>) -> Result<(), AndromedaError> {
        // Preserve body @context as jsonldContext — entity subs host compound contexts.
        // The body context comes from ContextSpec (set by the JsonLd extractor), not
        // from the payload directly, since the extractor strips @context before
        // deserialisation.
        if payload.get("jsonldContext").is_none()
            && let Some(ctx_val) = context.body_context()
        {
            self.ensure_context_urls_cached(ctx_val).await;

            let stored = match ctx_val {
                Value::String(s) => Some(Value::String(s.clone())), // clone: need owned String for new Value; s is borrowed from ctx_val
                Value::Array(arr) if arr.len() == 1 && arr[0].is_string() => Some(arr[0].clone()), // clone: extracting single element from array as owned Value
                Value::Array(_) | Value::Object(_) => {
                    let hosted_payload = serde_json::json!({ "@context": ctx_val });
                    match self.context_repo.create_implicitly(hosted_payload, base_url).await {
                        Ok(urn) => {
                            let stored_value = match base_url {
                                Some(b) => format!("{}/ngsi-ld/v1/jsonldContexts/{}", b.trim_end_matches('/'), urn),
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

        subscription_core::expand_common_subscription_terms(&self.engine, payload, context).await
    }

    async fn serialize_for_response(&self, subscription: &Subscription, context: &ContextSpec) -> Result<Value, AndromedaError> {
        let mut value = serde_json::to_value(subscription).map_err(|e| AndromedaError::subscription_validation(format!("Serialization failed: {}", e)))?;

        subscription_core::compact_subscription_iris(&self.engine, &mut value, context).await;
        subscription_core::fixup_integer_fields(&mut value);
        subscription_core::fixup_default_notification_trigger(&mut value);
        subscription_core::fixup_jsonld_context(&mut value);

        Ok(value)
    }

    fn validate_update_extra(&self, payload: &Value) -> Result<(), AndromedaError> {
        // Per spec 5.8.2.4: past expiresAt must be rejected for entity subscription updates
        subscription_core::validate_expires_at_not_in_past(payload)
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

impl EntitySubscriptionVariant {
    async fn ensure_context_urls_cached(&self, ctx_val: &Value) {
        let urls: Vec<&str> = match ctx_val {
            Value::String(s) => vec![s.as_str()],
            Value::Array(arr) => arr.iter().filter_map(|v| v.as_str()).collect(),
            _other => vec![],
        };

        for url in urls {
            if crate::jsonld::core_context::is_ngsi_ld_core_context_url(url) {
                continue;
            }
            match self.engine.resolve_context(url).await {
                Ok(payload) => {
                    if let Err(e) = self.context_repo.ensure_cached(url, payload).await {
                        warn!("Failed to cache context URL {}: {}", url, e);
                    }
                }
                Err(e) => {
                    warn!("Failed to resolve context URL {} for caching: {}", url, e);
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Public type alias — callers continue to use `SubscriptionService`
// ---------------------------------------------------------------------------

/// Entity subscription service combining shared CRUD logic with entity-specific behaviour.
pub type SubscriptionService = SubscriptionServiceCore<SubscriptionRepository, EntitySubscriptionVariant>;

impl SubscriptionService {
    /// Create a new entity subscription service backed by the given database pool.
    pub fn new(pool: &PgPool, engine: JsonLdEngine, sub_change_tx: Option<broadcast::Sender<SubscriptionChange>>) -> SubscriptionService {
        let repo = SubscriptionRepository::new(pool);
        let context_repo = JsonLdContextRepository::new(pool);
        debug!("Subscription service created!");
        SubscriptionService {
            repo,
            engine: engine.clone(), // clone: engine is shared between service and variant
            variant: EntitySubscriptionVariant { engine, context_repo },
            change_tx: sub_change_tx,
        }
    }
}
