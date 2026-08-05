use crate::{
    core::error::AndromedaError,
    repository::csource_subscription::repo::CsourceSubscriptionRepository,
    service::{
        subscription_base::{SubscriptionServiceCore, SubscriptionVariant},
        subscription_core,
    },
};
use andromeda_jsonld::{context_spec::ContextSpec, engine::processor::JsonLdEngine};
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_model::{notification::CsourceSubscriptionChange, subscription::Subscription};
use async_trait::async_trait;
use serde_json::Value;
use sqlx::postgres::PgPool;
use tokio::sync::broadcast;
use tracing::debug;

/// CSR-subscription-specific behaviour.
pub struct CsrSubscriptionVariant {
    engine: JsonLdEngine,
}

#[async_trait]
impl SubscriptionVariant for CsrSubscriptionVariant {
    type Change = CsourceSubscriptionChange;

    async fn expand_terms(&self, payload: &mut Value, context: &ContextSpec, _base_url: Option<&str>) -> Result<(), AndromedaError> {
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
                _other => Value::Null,
            };

            if jsonld_context_value != Value::Null
                && let Some(obj) = payload.as_object_mut()
            {
                let stored = match jsonld_context_value {
                    Value::String(s) => Value::String(s),
                    other => Value::String(other.to_string()),
                };
                obj.insert("jsonldContext".to_string(), stored);
            }
        }

        subscription_core::expand_common_subscription_terms(&self.engine, payload, context).await
    }

    async fn serialize_for_response(&self, subscription: &Subscription, context: &ContextSpec) -> Result<Value, AndromedaError> {
        let mut value = serde_json::to_value(subscription).map_err(|e| AndromedaError::subscription_validation(format!("Serialization failed: {}", e)))?;
        subscription_core::fixup_expired_status(&mut value, subscription.lifecycle.expires_at);
        subscription_core::compact_subscription_iris(&self.engine, &mut value, context).await;
        Ok(value)
    }

    fn validate_update_extra(&self, _payload: &Value) -> Result<(), AndromedaError> {
        // CSR subscription updates (5.11.3) do NOT reject past expiresAt
        Ok(())
    }

    fn use_dataset_id(&self) -> bool {
        false
    }

    fn use_entity_sub_merge_mode(&self) -> bool {
        false
    }

    fn make_upserted(&self, id: Uri) -> CsourceSubscriptionChange {
        CsourceSubscriptionChange::Upserted(id)
    }

    fn make_deleted(&self, id: Uri) -> CsourceSubscriptionChange {
        CsourceSubscriptionChange::Deleted(id)
    }
}

// ---------------------------------------------------------------------------
// Public type alias — callers continue to use `CsourceSubscriptionService`
// ---------------------------------------------------------------------------

/// Context source subscription service combining shared CRUD logic with CSR-specific behaviour.
pub type CsourceSubscriptionService = SubscriptionServiceCore<CsourceSubscriptionRepository, CsrSubscriptionVariant>;

impl CsourceSubscriptionService {
    /// Create a new context source subscription service backed by the given database pool.
    pub fn new(pool: &PgPool, engine: JsonLdEngine, sub_change_tx: Option<broadcast::Sender<CsourceSubscriptionChange>>) -> CsourceSubscriptionService {
        let repo = CsourceSubscriptionRepository::new(pool);
        debug!("CSR subscription service created!");
        CsourceSubscriptionService {
            repo,
            engine: engine.clone(), // clone: engine is shared between service and variant
            variant: CsrSubscriptionVariant { engine },
            change_tx: sub_change_tx,
        }
    }
}
