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
    deleted_members::DeletedMembers,
    endpoint_admission::{SubscriptionEndpointValidator, validate_endpoint},
    error::SubscriptionError,
    fields::SubscriptionWriteValues,
    forwarding_hook::SubscriptionForwardingHook,
    merged_validation::validate_merged_subscription,
    notification_context,
    request_validation,
    status::{compute_updated_status, requested_status_change},
    store::SubscriptionStore,
    subscription_context::SubscriptionContext,
    subscription_invariant::SubscriptionInvariant,
    variant::SubscriptionVariant,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_jsonld::{context_validation::admit_context_reference, engine::processor::JsonLdEngine, user_context_admission::AdmittedContext};
use andromeda_ngsi_ld_lexicon::{context_source_pseudonym::ContextSourcePseudonym, identifier::Identifier, ngsi_ld_urn::SubscriptionId};
use andromeda_ngsi_ld_model::{subscription::SubscriptionStatus, subscription_request::SubscriptionRequest};
use andromeda_ngsi_ld_query::{filter::language_targeting::LanguageTargeting, geo::admission::GeoQueryAdmission};
use andromeda_telemetry::subscription_ops::{self, SubscriptionOperation};
use chrono::{DateTime, Utc};
use futures::future::BoxFuture;
use iri_rs::IriBuf;
use std::sync::Arc;

/// Mint the identifier of a subscription whose client named none (clause
/// 5.8.1.4).
fn generate_id() -> Result<Identifier, SubscriptionError> {
    SubscriptionId::mint()
        .map(Identifier::from)
        .map_err(|error| SubscriptionInvariant::MintedIdentifier(error).into())
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
    /// What a written `geoQ` is admitted under.
    geo_q_admission: GeoQueryAdmission,
    /// The broker's `q_target_languages` level, which a written filter's
    /// target languages are admitted under.
    language_targeting: LanguageTargeting,
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
        geo_q_admission: GeoQueryAdmission,
        language_targeting: LanguageTargeting,
    ) -> SubscriptionService<V> {
        SubscriptionService {
            repo,
            engine,
            variant,
            change_sink,
            endpoint_validator,
            forwarding_hook: None,
            geo_q_admission,
            language_targeting,
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

    /// Admit the user `@context` the `jsonldContext` a request names, if it
    /// names one: clauses 5.8.1.4 and 5.8.2.4 refuse an unavailable one with
    /// `LdContextNotAvailable` and an invalid one with `BadRequestData`, and
    /// clause 5.5.7 refuses one declaring a JSON-LD Scoped Context, which could
    /// modify Core terms while a notification is compacted under it.
    async fn admit_jsonld_context(&self, jsonld_context: Option<&IriBuf>) -> Result<(), SubscriptionError> {
        if let Some(reference) = jsonld_context {
            admit_context_reference(&self.engine, reference).await?;
        }
        Ok(())
    }

    /// Create a new subscription from the given request body.
    ///
    /// `encountered_sources` carries the pseudonyms named in the request's
    /// `Via` chain (Table 6.3.18-2): a forwarded subscription must not spawn a
    /// companion copy back to a context source it already passed through, so the
    /// forwarding hook excludes their registrations. A local request supplies an
    /// empty slice.
    ///
    /// # Errors
    ///
    /// Returns [`SubscriptionError::Validation`] when the body does not
    /// satisfy clause 5.8.1 or when the endpoint names a transport this broker
    /// cannot deliver over; [`SubscriptionError::AlreadyExists`] when the
    /// requested `id` is taken; [`SubscriptionError::JsonLd`] when the request
    /// `@context` or the subscription's own `jsonldContext` cannot be resolved
    /// or the terms cannot be expanded; [`SubscriptionError::ContextHosting`]
    /// when the `@context` applicable to the request needs a hosted wrapper
    /// that cannot be stored (clause 5.8.1.4); [`SubscriptionError::Persistence`]
    /// when the store rejects the write. A failing forwarding hook is logged
    /// rather than returned: the local subscription is created either way.
    pub async fn create(
        &self,
        target: &ContextTarget,
        mut request: SubscriptionRequest,
        subscription_context: SubscriptionContext<'_>,
        encountered_sources: &[ContextSourcePseudonym],
    ) -> Result<Identifier, SubscriptionError> {
        let context = subscription_context.spec();
        let result: Result<Identifier, SubscriptionError> = async {
            request_validation::validate_create_request(&request, self.geo_q_admission, self.language_targeting)?;

            validate_endpoint(
                request.notification.as_ref().and_then(|notification| notification.endpoint.as_ref()),
                self.endpoint_validator.as_ref(),
            )?;

            let id = match request.id.take() {
                Some(provided_id) => {
                    if self.repo.exists(target, &provided_id).await? {
                        return Err(SubscriptionError::already_exists(&provided_id));
                    }
                    provided_id
                }
                None => generate_id()?,
            };

            self.admit_jsonld_context(request.jsonld_context.as_set()).await?;

            // Clause 5.8.1.4 (and clause 5.11.2.4 "as described in clause
            // 5.8.1.4"): a subscription created without `jsonldContext` has the
            // member initialised from the `@context` applicable to it, so by the
            // time the request is read the member reflects that initialisation.
            notification_context::initialize_notification_context(&mut request.jsonld_context, subscription_context, &self.engine).await?;
            self.variant.expand_terms(&mut request, context).await?;

            let status = compute_updated_status(SubscriptionStatus::Active, &requested_status_change(&request));
            let values = SubscriptionWriteValues::from_create(request, self.variant.dataset_id_policy())?;
            let fields = values.to_fields();

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

    /// Update an existing subscription with the given fragment, whose terms
    /// are expanded under `context`.
    ///
    /// The stored `jsonldContext` changes only when the fragment names one
    /// (clauses 5.8.2.4 and 5.5.8): initialising it from the applicable
    /// `@context` is a clause 5.8.1.4 creation step, so an update whose request
    /// merely names a context leaves the subscriber's choice in place and hosts
    /// nothing.
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
        mut request: SubscriptionRequest,
        context: &AdmittedContext,
    ) -> Result<(), SubscriptionError> {
        let result: Result<(), SubscriptionError> = async {
            validate_endpoint(
                request.notification.as_ref().and_then(|notification| notification.endpoint.as_ref()),
                self.endpoint_validator.as_ref(),
            )?;

            request_validation::validate_common_members(&request, self.geo_q_admission, self.language_targeting)?;
            self.variant.validate_update_extra(&request)?;
            request_validation::validate_notification_fragment(&request)?;

            self.admit_jsonld_context(request.jsonld_context.as_set()).await?;

            self.variant.expand_terms(&mut request, context).await?;

            let deleted = DeletedMembers::of(&request);
            let existing = self.get(target, id).await?;
            // Nothing is modified, so `modifiedAt` stays and no change is announced (clause 4.8).
            if request.is_empty() {
                return Ok(());
            }
            let existing_status = existing.lifecycle.standing.status;
            let status = compute_updated_status(existing_status, &requested_status_change(&request));
            let merged = SubscriptionWriteValues::merged(request, existing, self.variant.dataset_id_policy())?;
            validate_merged_subscription(&merged)?;
            let fields = merged.to_fields();

            let updated = self.repo.update_subscription(target, id, &fields, status).await?;
            if updated == 0 {
                return Err(SubscriptionError::not_found(id));
            }

            self.emit_change(self.variant.make_upserted(target.clone(), id.clone(), UpsertKind::Updated))
                .await; // clone: the change event owns the branch and identifier.
            if self.forwarding_hook.is_some() && target.permits_federation() {
                let merged = self.get(target, id).await?;
                self.run_hook(id, |hook| hook.updated(&merged, &deleted, context)).await;
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
