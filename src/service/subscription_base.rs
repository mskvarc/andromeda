//! Unified subscription service core.
//!
//! `SubscriptionServiceCore<R, V>` holds the CRUD logic shared by entity
//! subscriptions and CSR subscriptions.  Variant-specific behaviour is
//! delegated to the [`SubscriptionVariant`] trait.

use crate::{
    error::AndromedaError,
    service::{
        subscription::store::{StatusUpdateFields, SubscriptionStore},
        subscription_core,
        subscription_status::compute_updated_status,
    },
};
use andromeda_delivery::endpoint::transport::parse_endpoint_transport;
use andromeda_jsonld::{context_spec::ContextSpec, context_validation::validate_jsonld_context_value, engine::processor::JsonLdEngine};
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_model::{
    key_value_pair::KeyValuePair,
    subscription::{Subscription, SubscriptionStatus},
};
use andromeda_postgresql_context_store::subscription::repo::SubscriptionRepository;
use futures::future::BoxFuture;
use serde::Deserialize;
use serde_json::Value;

/// Reject a subscription whose endpoint names a transport this broker cannot
/// deliver over, or a `notifierInfo` it cannot read.
///
/// Runs against the raw payload rather than a parsed [`Subscription`] because
/// an update carries a fragment, and the same check has to hold for a create
/// and for a patch that touches only the endpoint.
fn validate_endpoint(payload: &Value) -> Result<(), AndromedaError> {
    let Some(endpoint) = payload.get("notification").and_then(|notification| notification.get("endpoint")) else {
        return Ok(());
    };
    let Some(endpoint_uri) = endpoint.get("uri").and_then(Value::as_str) else {
        return Ok(());
    };

    let notifier_info = match endpoint.get("notifierInfo") {
        Some(value) => Some(
            Vec::<KeyValuePair>::deserialize(value)
                .map_err(|error| AndromedaError::subscription_validation(format!("notifierInfo must be an array of key-value pairs: {error}")))?,
        ),
        None => None,
    };

    parse_endpoint_transport(endpoint_uri, notifier_info.as_deref())
        .map(|_transport| ())
        .map_err(|error| AndromedaError::subscription_validation(error.to_string()))
}

// ---------------------------------------------------------------------------
// Variant trait
// ---------------------------------------------------------------------------

/// Trait for variant-specific subscription behaviour (entity vs. CSR subscriptions).
pub trait SubscriptionVariant: Send + Sync {
    /// The change-event type emitted over the broadcast channel.
    type Change: Clone + Send + 'static;

    /// Expand subscription terms in-place (context hosting, IRI expansion).
    fn expand_terms<'life0, 'life1, 'life2, 'life3, 'async_trait>(
        &'life0 self,
        payload: &'life1 mut Value,
        context: &'life2 ContextSpec,
        base_url: Option<&'life3 str>,
    ) -> BoxFuture<'async_trait, Result<(), AndromedaError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
        'life3: 'async_trait,
        Self: 'async_trait;

    /// Serialize a subscription for an HTTP response (compaction, fixups).
    fn serialize_for_response<'life0, 'life1, 'life2, 'async_trait>(
        &'life0 self,
        subscription: &'life1 Subscription,
        context: &'life2 ContextSpec,
    ) -> BoxFuture<'async_trait, Result<Value, AndromedaError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
        Self: 'async_trait;

    /// Additional update-specific validation (e.g. expiresAt future check).
    fn validate_update_extra(&self, payload: &Value) -> Result<(), AndromedaError>;

    /// Whether the `datasetId` field should be extracted from the payload.
    fn use_dataset_id(&self) -> bool;

    /// Whether `merge_update_fields` should use entity-sub mode.
    fn use_entity_sub_merge_mode(&self) -> bool;

    /// Build an "upserted" change event.
    fn make_upserted(&self, id: Uri) -> Self::Change;

    /// Build a "deleted" change event.
    fn make_deleted(&self, id: Uri) -> Self::Change;
}

// ---------------------------------------------------------------------------
// Core service
// ---------------------------------------------------------------------------

/// Generic subscription service providing CRUD operations shared by entity and CSR subscriptions.
pub struct SubscriptionServiceCore<R, V: SubscriptionVariant> {
    /// Subscription persistence backend.
    pub(crate) repo: R,
    /// JSON-LD processing engine for term expansion and compaction.
    pub(crate) engine: JsonLdEngine,
    /// Variant-specific behaviour delegate.
    pub(crate) variant: V,
    /// Optional broadcast channel for subscription change events.
    pub(crate) change_tx: Option<tokio::sync::broadcast::Sender<V::Change>>,
}

impl<R, V> SubscriptionServiceCore<R, V>
where
    R: SubscriptionStore,
    V: SubscriptionVariant,
{
    fn emit_change(&self, change: V::Change) {
        if let Some(ref tx) = self.change_tx {
            let _ = tx.send(change);
        }
    }

    /// Create a new subscription from the given JSON payload.
    pub async fn create(&self, mut payload: Value, context: &ContextSpec, base_url: Option<&str>) -> Result<String, AndromedaError> {
        subscription_core::validation::validate_create_payload(&payload)?;

        validate_endpoint(&payload)?;

        let id = match payload.get("id").and_then(|v| v.as_str()) {
            Some(provided_id) => {
                if provided_id.trim().is_empty() || provided_id.parse::<Uri>().is_err() {
                    return Err(AndromedaError::subscription_validation("Subscription id must be present and a valid URI"));
                }
                if self.repo.exists(provided_id).await? {
                    return Err(AndromedaError::subscription_already_exists(provided_id));
                }
                provided_id.to_string()
            }
            None => SubscriptionRepository::generate_id()?,
        };

        if let Some(ctx_value) = payload.get("jsonldContext") {
            validate_jsonld_context_value(&self.engine, ctx_value).await?;
        }

        self.variant.expand_terms(&mut payload, context, base_url).await?;

        let watched_attributes_vec: Option<Vec<String>> = payload
            .get("watchedAttributes")
            .map(|v| {
                serde_json::from_value(v.clone()).map_err(|e| AndromedaError::subscription_validation(format!("Invalid 'watchedAttributes' value: {}", e)))
            }) // clone: serde_json::from_value consumes the Value
            .transpose()?;
        let notification_trigger_vec: Option<Vec<String>> = payload
            .get("notificationTrigger")
            .map(|v| {
                serde_json::from_value(v.clone()).map_err(|e| AndromedaError::subscription_validation(format!("Invalid 'notificationTrigger' value: {}", e)))
            }) // clone: serde_json::from_value consumes the Value
            .transpose()?;
        let dataset_id_vec: Option<Vec<String>> = if self.variant.use_dataset_id() {
            payload
                .get("datasetId")
                .map(|v| serde_json::from_value(v.clone()).map_err(|e| AndromedaError::subscription_validation(format!("Invalid 'datasetId' value: {}", e)))) // clone: serde_json::from_value consumes the Value
                .transpose()?
        } else {
            None
        };

        let fields = subscription_core::fields::extract_create_fields(&payload, &watched_attributes_vec, &notification_trigger_vec, &dataset_id_vec);
        subscription_core::fields::validate_persisted_shape(&fields)?;

        let status = compute_updated_status(
            SubscriptionStatus::Active,
            &StatusUpdateFields {
                is_active: if payload.get("isActive").is_some() { Some(fields.is_active) } else { None },
                expires_at: if fields.expires_at.is_some() { Some(fields.expires_at) } else { None },
            },
        );

        self.repo.create_subscription(&id, &fields, status).await?;
        match id.parse::<Uri>() {
            Ok(id_uri) => self.emit_change(self.variant.make_upserted(id_uri)),
            Err(_) => {
                tracing::error!(subscription_id = %id, event = "subscription_uri_parse_failed");
            }
        }
        Ok(id)
    }

    /// Retrieve a subscription by its identifier.
    pub async fn get(&self, id: &str) -> Result<Subscription, AndromedaError> {
        if id.is_empty() || id.parse::<Uri>().is_err() {
            return Err(AndromedaError::subscription_validation("Subscription id must be a valid URI"));
        }
        self.repo.get(id).await?.ok_or_else(|| AndromedaError::subscription_not_found(id))
    }

    /// Retrieve a subscription and serialize it for an HTTP response.
    pub async fn get_for_response(&self, id: &str, context: &ContextSpec) -> Result<Value, AndromedaError> {
        let subscription = self.get(id).await?;
        self.variant.serialize_for_response(&subscription, context).await
    }

    /// List subscriptions with pagination and serialize them for an HTTP response.
    pub async fn list_for_response(&self, limit: Option<i64>, offset: Option<i64>, context: &ContextSpec) -> Result<(Vec<Value>, i64), AndromedaError> {
        let (subscriptions, total) = self.repo.list(limit, offset).await?;
        let mut compacted = Vec::with_capacity(subscriptions.len());
        for subscription in subscriptions {
            compacted.push(self.variant.serialize_for_response(&subscription, context).await?);
        }
        Ok((compacted, total))
    }

    /// Update an existing subscription with the given fragment payload.
    pub async fn update(&self, id: &str, mut payload: Value, context: &ContextSpec, base_url: Option<&str>) -> Result<(), AndromedaError> {
        if id.is_empty() || id.parse::<Uri>().is_err() {
            return Err(AndromedaError::subscription_validation("Subscription id must be present and a valid URI"));
        }

        validate_endpoint(&payload)?;

        subscription_core::validation::validate_common_fields(&payload)?;
        self.variant.validate_update_extra(&payload)?;
        subscription_core::validation::validate_notification_fragment(&payload)?;

        if let Some(ctx_value) = payload.get("jsonldContext") {
            validate_jsonld_context_value(&self.engine, ctx_value).await?;
        }

        self.variant.expand_terms(&mut payload, context, base_url).await?;

        let existing = self.repo.get(id).await?.ok_or_else(|| AndromedaError::subscription_not_found(id))?;
        let existing_value =
            serde_json::to_value(&existing).map_err(|e| AndromedaError::internal(format!("Failed to serialize existing subscription: {}", e)))?;

        let merged = subscription_core::fields::merge_update_fields(&payload, &existing, &existing_value, self.variant.use_entity_sub_merge_mode())?;
        let fields = merged.to_fields();
        subscription_core::fields::validate_persisted_shape(&fields)?;

        let status = compute_updated_status(
            existing.lifecycle.status,
            &StatusUpdateFields {
                is_active: if payload.get("isActive").is_some() { Some(fields.is_active) } else { None },
                expires_at: if payload.get("expiresAt").is_some() { Some(fields.expires_at) } else { None },
            },
        );

        let updated = self.repo.update_subscription(id, &fields, status).await?;
        if updated == 0 {
            return Err(AndromedaError::subscription_not_found(id));
        }

        match id.parse::<Uri>() {
            Ok(id_uri) => self.emit_change(self.variant.make_upserted(id_uri)),
            Err(_) => {
                tracing::error!(subscription_id = %id, event = "subscription_uri_parse_failed");
            }
        }
        Ok(())
    }

    /// Delete a subscription by its identifier.
    pub async fn delete(&self, id: &str) -> Result<(), AndromedaError> {
        if id.is_empty() || id.parse::<Uri>().is_err() {
            return Err(AndromedaError::subscription_validation("Subscription id must be present and a valid URI"));
        }

        let deleted = self.repo.delete(id).await?;
        if deleted == 0 {
            return Err(AndromedaError::subscription_not_found(id));
        }

        match id.parse::<Uri>() {
            Ok(id_uri) => self.emit_change(self.variant.make_deleted(id_uri)),
            Err(_) => {
                tracing::error!(subscription_id = %id, event = "subscription_uri_parse_failed");
            }
        }
        Ok(())
    }
}
