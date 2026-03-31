use crate::{
    domain::{notification::CsourceSubscriptionChange, subscription::Subscription},
    error::AndromedaError,
    jsonld::{ContextSpec, JsonLdEngine},
    repository::csource_subscription::CsourceSubscriptionRepository,
    service::{
        subscription_base::{SubscriptionServiceCore, SubscriptionVariant},
        subscription_core,
    },
};
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
        // Store @context as jsonldContext (simplified — no context hosting)
        if payload.get("jsonldContext").is_none()
            && let Some(ctx_val) = payload.get("@context")
        {
            let jsonld_context_value = match ctx_val {
                Value::String(s) => Value::String(s.clone()),
                Value::Array(_) | Value::Object(_) => ctx_val.clone(),
                _ => Value::Null,
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
        let mut value = serde_json::to_value(subscription).map_err(|e| AndromedaError::bad_request_data(format!("Serialization failed: {}", e)))?;
        subscription_core::fixup_expired_status(&mut value, subscription.expires_at);
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

    fn make_upserted(&self, id: String) -> CsourceSubscriptionChange {
        CsourceSubscriptionChange::Upserted(id)
    }

    fn make_deleted(&self, id: String) -> CsourceSubscriptionChange {
        CsourceSubscriptionChange::Deleted(id)
    }
}

// ---------------------------------------------------------------------------
// Public type alias — callers continue to use `CsourceSubscriptionService`
// ---------------------------------------------------------------------------

pub type CsourceSubscriptionService = SubscriptionServiceCore<CsourceSubscriptionRepository, CsrSubscriptionVariant>;

impl CsourceSubscriptionService {
    pub fn new(pool: &PgPool, engine: JsonLdEngine, sub_change_tx: Option<broadcast::Sender<CsourceSubscriptionChange>>) -> Self {
        let repo = CsourceSubscriptionRepository::new(pool);
        debug!("CSR subscription service created!");
        Self {
            repo,
            engine: engine.clone(),
            variant: CsrSubscriptionVariant { engine },
            change_tx: sub_change_tx,
        }
    }
}
