//! The subscription lifecycle both kinds of subscription share (ETSI GS CIM
//! 009 clauses 5.8.1 to 5.8.5 and 5.11.2 to 5.11.6).
//!
//! `SubscriptionService<V>` owns create, retrieve, list, update, delete and
//! expiry for entity subscriptions and Context Source Registration
//! Subscriptions alike; what the two do differently is declared by
//! `crate::variant::SubscriptionVariant` and nothing else. Retrieval and
//! listing live in `read`; this module holds the service and its mutations.

use crate::{
    change::{SubscriptionChangeSink, UpsertKind},
    endpoint_admission::{SubscriptionEndpointValidator, validate_endpoint},
    error::SubscriptionError,
    fields,
    forwarding_hook::SubscriptionForwardingHook,
    merged_validation::validate_merged_subscription,
    status::compute_updated_status,
    store::{StatusUpdateFields, SubscriptionStore},
    subscription_invariant::SubscriptionInvariant,
    subscription_validation_reason::SubscriptionValidationReason,
    validation,
    variant::SubscriptionVariant,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_jsonld::{
    context_spec::ContextSpec,
    context_validation::{validate_context_spec, validate_jsonld_context_value},
    engine::processor::JsonLdEngine,
};
use andromeda_ngsi_ld_lexicon::{context_source_pseudonym::ContextSourcePseudonym, identifier::Identifier, uri::NGSI_LD_URN_NID};
use andromeda_ngsi_ld_model::subscription::SubscriptionStatus;
use andromeda_telemetry::subscription_ops::{self, SubscriptionOperation};
use chrono::{DateTime, Utc};
use futures::future::BoxFuture;
use serde_json::Value;
use std::sync::Arc;
use url::Url;
use urn_rs::UrnBuilder;
use uuid::Uuid;

fn generate_id() -> Result<Identifier, SubscriptionError> {
    let urn = UrnBuilder::new(NGSI_LD_URN_NID, &format!("Subscription:{}", Uuid::new_v4())).build()?;
    Identifier::try_from(urn).map_err(|error| SubscriptionInvariant::MintedIdentifier(error).into())
}

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
    async fn emit_change(&self, change: V::Change) {
        if let Some(ref sink) = self.change_sink {
            sink.publish(change).await;
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
    async fn run_hook<'a>(&'a self, id: &Identifier, step: impl FnOnce(&'a dyn SubscriptionForwardingHook) -> BoxFuture<'a, Result<(), SubscriptionError>>) {
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
    ///
    /// # Errors
    ///
    /// Returns [`SubscriptionError::Validation`] when the payload does not
    /// satisfy clause 5.8.1, when its `id` is not a URI, or when the endpoint
    /// names a transport this broker cannot deliver over;
    /// [`SubscriptionError::AlreadyExists`] when the requested `id` is taken;
    /// [`SubscriptionError::JsonLd`] when the request `@context` or the
    /// subscription's own `jsonldContext` cannot be resolved or the terms
    /// cannot be expanded; [`SubscriptionError::Persistence`] when the store
    /// rejects the write. A failing forwarding hook is logged rather than
    /// returned: the local subscription is created either way.
    pub async fn create(
        &self,
        target: &ContextTarget,
        mut payload: Value,
        context: &ContextSpec,
        base_url: Option<&Url>,
        encountered_sources: &[ContextSourcePseudonym],
    ) -> Result<Identifier, SubscriptionError> {
        let result: Result<Identifier, SubscriptionError> = async {
            validation::validate_create_payload(&payload)?;

            validate_endpoint(&payload, self.endpoint_validator.as_ref())?;

            let id = match payload.get("id").and_then(|v| v.as_str()) {
                Some(provided_id) => {
                    let provided_id = provided_id
                        .parse::<Identifier>()
                        .map_err(|source| SubscriptionError::Validation(SubscriptionValidationReason::SubscriptionId { source }))?;
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

            // Clause 5.8.2.4: `expand_terms` initialises `jsonldContext` from the
            // `@context` applicable at creation when the request named none itself,
            // so by the time the payload is parsed the member reflects that fallback.
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
            self.emit_change(self.variant.make_upserted(target.clone(), id.clone(), UpsertKind::Created))
                .await; // clone: the caller and change event each own the branch and identifier.
            if self.forwarding_hook.is_some() && target.permits_federation() {
                let created = self.get(target, &id).await?;
                self.run_hook(&id, |hook| hook.created(&created, context, encountered_sources)).await;
            }
            Ok(id)
        }
        .await;
        subscription_ops::record_result(SubscriptionOperation::Create, &result);
        result
    }

    /// Update an existing subscription with the given fragment payload.
    ///
    /// # Errors
    ///
    /// Returns [`SubscriptionError::Validation`] when the fragment does not
    /// satisfy clause 5.8.2 or names an endpoint this broker cannot deliver
    /// over; [`SubscriptionError::NotFound`] when `target` holds no
    /// subscription under `id`; [`SubscriptionError::JsonLd`] when the request
    /// `@context` or the subscription's own `jsonldContext` cannot be resolved
    /// or the terms cannot be expanded; [`SubscriptionError::Persistence`]
    /// when the store rejects the write. A failing forwarding hook is logged
    /// rather than returned: the local subscription is updated either way.
    pub async fn update(
        &self,
        target: &ContextTarget,
        id: &Identifier,
        mut payload: Value,
        context: &ContextSpec,
        base_url: Option<&Url>,
    ) -> Result<(), SubscriptionError> {
        let result: Result<(), SubscriptionError> = async {
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
            validate_merged_subscription(&merged)?;
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

            self.emit_change(self.variant.make_upserted(target.clone(), id.clone(), UpsertKind::Updated))
                .await; // clone: the change event owns the branch and identifier.
            if self.forwarding_hook.is_some() && target.permits_federation() {
                let merged = self.get(target, id).await?;
                self.run_hook(id, |hook| hook.updated(&merged, context)).await;
            }
            Ok(())
        }
        .await;
        subscription_ops::record_result(SubscriptionOperation::Update, &result);
        result
    }

    /// Delete a subscription by its identifier.
    ///
    /// # Errors
    ///
    /// Returns [`SubscriptionError::NotFound`] when `target` holds no
    /// subscription under `id`, and [`SubscriptionError::Persistence`] when
    /// the store rejects the delete. A failing forwarding hook is logged
    /// rather than returned: the local subscription is deleted either way.
    pub async fn delete(&self, target: &ContextTarget, id: &Identifier) -> Result<(), SubscriptionError> {
        let result: Result<(), SubscriptionError> = async {
            // Captured before the row goes: the store deletes the forwarding
            // mappings together with the subscription.
            let teardown = match self.forwarding_hook.as_deref() {
                Some(hook) if target.permits_federation() => hook
                    .teardown_of(id)
                    .await
                    .inspect_err(|error| tracing::warn!(subscription = %id, %error, "subscription forwarding teardown could not be captured"))
                    .ok(),
                Some(_) | None => None,
            };
            let deleted = self.repo.delete(target, id).await?;
            if deleted == 0 {
                return Err(SubscriptionError::not_found(id));
            }

            self.emit_change(self.variant.make_deleted(target.clone(), id.clone())).await; // clone: the change event owns the branch and identifier.
            if let Some(teardown) = &teardown {
                self.run_hook(id, |hook| hook.deleted(id, teardown)).await;
            }
            Ok(())
        }
        .await;
        subscription_ops::record_result(SubscriptionOperation::Delete, &result);
        result
    }

    /// Transition subscriptions due at `cutoff` and publish one variant-owned deletion change for each.
    pub(crate) async fn expire_due(&self, target: &ContextTarget, cutoff: DateTime<Utc>) -> Result<Vec<Identifier>, SubscriptionError> {
        let identifiers = self.repo.expire_due(target, cutoff).await?;
        for identifier in &identifiers {
            self.emit_change(self.variant.make_deleted(target.clone(), identifier.clone())).await; // clone: the policy outcome and change sink each own the branch and identifier.
        }
        Ok(identifiers)
    }
}

/// Retrieving and listing subscriptions.
mod read;
#[cfg(test)]
mod tests;
