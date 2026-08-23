//! Unified subscription service core.
//!
//! `SubscriptionService<V>` holds the CRUD logic shared by entity
//! subscriptions and CSR subscriptions.  Variant-specific behaviour is
//! delegated to the [`SubscriptionVariant`] trait.

use crate::{
    change::SubscriptionChangeSink,
    error::SubscriptionError,
    fields,
    forwarding_hook::SubscriptionForwardingHook,
    status::compute_updated_status,
    store::{StatusUpdateFields, SubscriptionStore},
    validation,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_jsonld::{
    context_spec::ContextSpec,
    context_validation::{validate_context_spec, validate_jsonld_context_value},
    engine::processor::JsonLdEngine,
};
use andromeda_ngsi_ld_lexicon::{
    context_source_pseudonym::ContextSourcePseudonym,
    uri::{NGSI_LD_URN_NID, Uri},
};
use andromeda_ngsi_ld_model::{
    key_value_pair::KeyValuePair,
    subscription::{Subscription, SubscriptionStatus},
};
use chrono::{DateTime, Utc};
use futures::future::BoxFuture;
use serde::Deserialize;
use serde_json::Value;
use std::sync::Arc;
use url::Url;
use urn_rs::UrnBuilder;
use uuid::Uuid;

/// Reject a subscription whose endpoint names a transport this broker cannot
/// deliver over, or a `notifierInfo` it cannot read.
///
/// Runs against the raw payload rather than a parsed [`Subscription`] because
/// an update carries a fragment, and the same check has to hold for a create
/// and for a patch that touches only the endpoint.
fn validate_endpoint(payload: &Value, validator: &dyn SubscriptionEndpointValidator) -> Result<(), SubscriptionError> {
    let Some(endpoint) = payload.get("notification").and_then(|notification| notification.get("endpoint")) else {
        return Ok(());
    };
    let Some(endpoint_uri) = endpoint.get("uri").and_then(Value::as_str) else {
        return Ok(());
    };
    let endpoint_uri = Url::parse(endpoint_uri).map_err(|error| SubscriptionError::validation(error.to_string()))?;

    let notifier_info = match endpoint.get("notifierInfo") {
        Some(value) => Some(
            Vec::<KeyValuePair>::deserialize(value)
                .map_err(|error| SubscriptionError::validation(format!("notifierInfo must be an array of key-value pairs: {error}")))?,
        ),
        None => None,
    };

    validator.validate(&endpoint_uri, notifier_info.as_deref())
}

/// Validates whether endpoint configuration can be served by the selected delivery bindings.
pub trait SubscriptionEndpointValidator: Send + Sync + 'static {
    /// Validate one endpoint URL and its optional notifier metadata.
    fn validate(&self, endpoint: &Url, notifier_info: Option<&[KeyValuePair]>) -> Result<(), SubscriptionError>;
}

/// Controls whether a subscription variant persists clause 5.8 `datasetId`
/// filtering.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DatasetIdPolicy {
    /// Retain validated dataset identifiers for entity subscriptions.
    Retain,
    /// Discard dataset identifiers for context-source subscriptions, whose
    /// contract does not define this filter.
    Discard,
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
        base_url: Option<&'life3 Url>,
    ) -> BoxFuture<'async_trait, Result<(), SubscriptionError>>
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
    ) -> BoxFuture<'async_trait, Result<Value, SubscriptionError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
        Self: 'async_trait;

    /// Additional update-specific validation (e.g. expiresAt future check).
    fn validate_update_extra(&self, payload: &Value) -> Result<(), SubscriptionError>;

    /// Select how this subscription variant handles the `datasetId` field.
    fn dataset_id_policy(&self) -> DatasetIdPolicy;

    /// Build an "upserted" change event.
    fn make_upserted(&self, target: ContextTarget, id: Uri) -> Self::Change;

    /// Build a "deleted" change event.
    fn make_deleted(&self, target: ContextTarget, id: Uri) -> Self::Change;
}

fn generate_id() -> Result<Uri, SubscriptionError> {
    let urn = UrnBuilder::new(NGSI_LD_URN_NID, &format!("Subscription:{}", Uuid::new_v4())).build()?;
    Ok(Uri::from(urn))
}

// ---------------------------------------------------------------------------
// Core service
// ---------------------------------------------------------------------------

/// Generic subscription service providing CRUD operations shared by entity and CSR subscriptions.
pub struct SubscriptionService<V: SubscriptionVariant> {
    /// Subscription persistence backend.
    repo: Arc<dyn SubscriptionStore>,
    /// JSON-LD processing engine for term expansion and compaction.
    engine: JsonLdEngine,
    /// Variant-specific behaviour delegate.
    variant: V,
    /// Optional sink for completed subscription changes.
    change_sink: Option<Arc<dyn SubscriptionChangeSink<V::Change>>>,
    endpoint_validator: Arc<dyn SubscriptionEndpointValidator>,
    /// The distributed side of the lifecycle (clauses 5.8.1.4, 5.8.2.4,
    /// 5.8.5.4), absent for the variants that have none.
    forwarding_hook: Option<Arc<dyn SubscriptionForwardingHook>>,
}

impl<V> SubscriptionService<V>
where
    V: SubscriptionVariant,
{
    fn emit_change(&self, change: V::Change) {
        if let Some(ref sink) = self.change_sink {
            sink.publish(change);
        }
    }

    /// Construct subscription lifecycle policy around one persistence port.
    pub fn new(
        repo: Arc<dyn SubscriptionStore>,
        engine: JsonLdEngine,
        variant: V,
        change_sink: Option<Arc<dyn SubscriptionChangeSink<V::Change>>>,
        endpoint_validator: Arc<dyn SubscriptionEndpointValidator>,
    ) -> SubscriptionService<V> {
        SubscriptionService {
            repo,
            engine,
            variant,
            change_sink,
            endpoint_validator,
            forwarding_hook: None,
        }
    }

    /// Attach the distributed side of the lifecycle.
    #[must_use]
    pub fn with_forwarding_hook(mut self, hook: Arc<dyn SubscriptionForwardingHook>) -> SubscriptionService<V> {
        self.forwarding_hook = Some(hook);
        self
    }

    /// Run one hook step after the local change committed. The local
    /// subscription is valid whatever the distributed side does, so a failing
    /// hook is logged rather than turned into a failed request.
    async fn run_hook<'a>(&'a self, id: &Uri, step: impl FnOnce(&'a dyn SubscriptionForwardingHook) -> BoxFuture<'a, Result<(), SubscriptionError>>) {
        let Some(hook) = self.forwarding_hook.as_deref() else {
            return;
        };
        if let Err(error) = step(hook).await {
            tracing::warn!(subscription = %id, %error, "subscription forwarding hook failed");
        }
    }

    /// Create a new subscription from the given JSON payload.
    ///
    /// `encountered_sources` carries the pseudonyms named in the request's
    /// `Via` chain (Table 6.3.18-2): a forwarded subscription must not spawn a
    /// companion copy back to a context source it already passed through, so the
    /// forwarding hook excludes their registrations. A local request supplies an
    /// empty slice.
    pub async fn create(
        &self,
        target: &ContextTarget,
        mut payload: Value,
        context: &ContextSpec,
        base_url: Option<&Url>,
        encountered_sources: &[ContextSourcePseudonym],
    ) -> Result<Uri, SubscriptionError> {
        validation::validate_create_payload(&payload)?;

        validate_endpoint(&payload, self.endpoint_validator.as_ref())?;

        let id = match payload.get("id").and_then(|v| v.as_str()) {
            Some(provided_id) => {
                let provided_id = provided_id
                    .parse::<Uri>()
                    .map_err(|error| SubscriptionError::validation(format!("Subscription id must be a valid URI: {error}")))?;
                if self.repo.exists(target, &provided_id).await? {
                    return Err(SubscriptionError::already_exists(&provided_id));
                }
                provided_id
            }
            None => generate_id()?,
        };

        if let Some(ctx_value) = payload.get("jsonldContext") {
            validate_jsonld_context_value(&self.engine, ctx_value).await?;
        }
        validate_context_spec(&self.engine, context).await?;

        self.variant.expand_terms(&mut payload, context, base_url).await?;

        let values = fields::extract_create_fields(&payload, self.variant.dataset_id_policy())?;
        let fields = values.to_fields();

        let status = compute_updated_status(
            SubscriptionStatus::Active,
            &StatusUpdateFields {
                is_active: if payload.get("isActive").is_some() { Some(fields.is_active) } else { None },
                expires_at: if fields.expires_at.is_some() { Some(fields.expires_at) } else { None },
            },
        );

        self.repo.create_subscription(target, &id, &fields, status).await?;
        self.emit_change(self.variant.make_upserted(target.clone(), id.clone())); // clone: the caller and change event each own the branch and identifier.
        if self.forwarding_hook.is_some() && target.permits_federation() {
            let created = self.get(target, &id).await?;
            self.run_hook(&id, |hook| hook.created(&created, context, encountered_sources)).await;
        }
        Ok(id)
    }

    /// Retrieve a subscription by its identifier.
    ///
    /// The store-assigned key is dropped here: every caller of this method
    /// renders or edits the resource, and the resource is addressed by its URI.
    pub async fn get(&self, target: &ContextTarget, id: &Uri) -> Result<Subscription, SubscriptionError> {
        self.repo
            .get(target, id)
            .await?
            .map(|stored| stored.subscription)
            .ok_or_else(|| SubscriptionError::not_found(id))
    }

    /// Retrieve a subscription and serialize it for an HTTP response.
    pub async fn get_for_response(&self, target: &ContextTarget, id: &Uri, context: &ContextSpec) -> Result<Value, SubscriptionError> {
        let subscription = self.get(target, id).await?;
        self.variant.serialize_for_response(&subscription, context).await
    }

    /// List subscriptions with pagination and serialize them for an HTTP response.
    pub async fn list_for_response(
        &self,
        target: &ContextTarget,
        limit: Option<i64>,
        offset: Option<i64>,
        context: &ContextSpec,
    ) -> Result<(Vec<Value>, i64), SubscriptionError> {
        let (subscriptions, total) = self.repo.list(target, limit, offset).await?;
        let mut compacted = Vec::with_capacity(subscriptions.len());
        for subscription in subscriptions {
            compacted.push(self.variant.serialize_for_response(&subscription, context).await?);
        }
        Ok((compacted, total))
    }

    /// Update an existing subscription with the given fragment payload.
    pub async fn update(
        &self,
        target: &ContextTarget,
        id: &Uri,
        mut payload: Value,
        context: &ContextSpec,
        base_url: Option<&Url>,
    ) -> Result<(), SubscriptionError> {
        validate_endpoint(&payload, self.endpoint_validator.as_ref())?;

        validation::validate_common_fields(&payload)?;
        self.variant.validate_update_extra(&payload)?;
        validation::validate_notification_fragment(&payload)?;

        if let Some(ctx_value) = payload.get("jsonldContext") {
            validate_jsonld_context_value(&self.engine, ctx_value).await?;
        }
        validate_context_spec(&self.engine, context).await?;

        self.variant.expand_terms(&mut payload, context, base_url).await?;

        let existing = self.get(target, id).await?;
        let existing_status = existing.lifecycle.status;
        let merged = fields::merge_update_fields(&payload, existing, self.variant.dataset_id_policy())?;
        let fields = merged.to_fields();

        let status = compute_updated_status(
            existing_status,
            &StatusUpdateFields {
                is_active: if payload.get("isActive").is_some() { Some(fields.is_active) } else { None },
                expires_at: if payload.get("expiresAt").is_some() { Some(fields.expires_at) } else { None },
            },
        );

        let updated = self.repo.update_subscription(target, id, &fields, status).await?;
        if updated == 0 {
            return Err(SubscriptionError::not_found(id));
        }

        self.emit_change(self.variant.make_upserted(target.clone(), id.clone())); // clone: the change event owns the branch and identifier.
        if self.forwarding_hook.is_some() && target.permits_federation() {
            let merged = self.get(target, id).await?;
            self.run_hook(id, |hook| hook.updated(&merged, context)).await;
        }
        Ok(())
    }

    /// Delete a subscription by its identifier.
    pub async fn delete(&self, target: &ContextTarget, id: &Uri) -> Result<(), SubscriptionError> {
        let deleted = self.repo.delete(target, id).await?;
        if deleted == 0 {
            return Err(SubscriptionError::not_found(id));
        }

        self.emit_change(self.variant.make_deleted(target.clone(), id.clone())); // clone: the change event owns the branch and identifier.
        if target.permits_federation() {
            self.run_hook(id, |hook| hook.deleted(id)).await;
        }
        Ok(())
    }

    /// Transition subscriptions due at `cutoff` and publish one variant-owned deletion change for each.
    pub(crate) async fn expire_due(&self, target: &ContextTarget, cutoff: DateTime<Utc>) -> Result<Vec<Uri>, SubscriptionError> {
        let identifiers = self.repo.expire_due(target, cutoff).await?;
        for identifier in &identifiers {
            self.emit_change(self.variant.make_deleted(target.clone(), identifier.clone())); // clone: the policy outcome and change sink each own the branch and identifier.
        }
        Ok(identifiers)
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        error::SubscriptionError,
        lifecycle::{SubscriptionEndpointValidator, generate_id, validate_endpoint},
    };
    use andromeda_ngsi_ld_model::key_value_pair::KeyValuePair;
    use url::Url;

    struct RejectingEndpointValidator;

    impl SubscriptionEndpointValidator for RejectingEndpointValidator {
        fn validate(&self, _endpoint: &Url, _notifier_info: Option<&[KeyValuePair]>) -> Result<(), SubscriptionError> {
            Err(SubscriptionError::validation("unsupported endpoint"))
        }
    }

    #[test]
    fn generated_subscription_identifiers_are_ngsi_ld_urns() {
        let identifier = generate_id();
        assert!(matches!(
            identifier,
            Ok(identifier) if identifier.as_str().starts_with("urn:ngsi-ld:Subscription:")
        ));
    }

    #[test]
    fn endpoint_validation_rejects_an_invalid_url_before_calling_the_binding() {
        let payload = serde_json::json!({"notification": {"endpoint": {"uri": "://invalid"}}});
        assert!(matches!(
            validate_endpoint(&payload, &RejectingEndpointValidator),
            Err(SubscriptionError::Validation { .. })
        ));
    }

    #[test]
    fn endpoint_validation_propagates_the_selected_binding_refusal() {
        let payload = serde_json::json!({"notification": {"endpoint": {"uri": "http://example.com/notify"}}});
        let error = validate_endpoint(&payload, &RejectingEndpointValidator);
        assert!(matches!(error, Err(SubscriptionError::Validation { detail }) if detail == "unsupported endpoint"));
    }
}
