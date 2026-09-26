//! Federated batch provision operations (clauses 5.6.7.4, 5.6.8.4, 5.6.9.4,
//! 5.6.10.4 and 5.6.20.4).
//!
//! Every batch operation forwards the same way: for each registration, the
//! client's array is narrowed to the entities the registration matches and the
//! attributes it covers, stripped of the attributes another exclusive (or, for
//! a non-redirect registration, redirect) registration owns, and emptied of
//! entities left without attributes. A registration advertising the batch
//! operation receives that array in one request; one advertising only the
//! single-entity operation is driven once per entity; a proxied registration
//! advertising neither fails every entity with a Conflict.

/// Reading a forwarded batch request's answer entity by entity.
mod batch_answer;
/// The clause 5.6.7.4 to 5.6.20.4 decision ladder for one registration.
mod fallback_plan;
/// Narrowing the client's array to what one registration receives.
mod narrowing;
/// The registrations and entities the batch forwarding tests share.
#[cfg(test)]
mod registration_fixture;
/// The request a single-entity fallback write sends.
mod single_write;

use crate::{
    FederationService,
    error::FederationError,
    forwarded_body::ForwardedBody,
    forwarding_url::forwarding_url,
    operation::ForwardingOperation,
    outcome::ForwardedWriteOutcome,
    provision::{ForwardWrite, is_proxied, proxied_mode},
    provision_batch::{
        batch_answer::batch_outcomes,
        fallback_plan::{FallbackPlan, SingleWrite, follow_up_write, plan_for},
        narrowing::{NarrowedEntity, narrow_input},
        single_write::single_write_request,
    },
    request::TriggeringRequest,
    term_resolver::TermResolver,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName, uri::Uri};
use andromeda_ngsi_ld_model::{
    expanded::mutation::selection::OverwritePolicy,
    registration::{ContextSourceRegistration, forwarding_mode::ForwardingMode},
};
use andromeda_ngsi_ld_request::options::UpsertMode;
use futures::{StreamExt, stream};
use iri_rs::IriBuf;
use serde_json::Value;
use std::collections::HashMap;
use tracing::debug;

/// The batch provision operation being forwarded, with the option that
/// decides its single-entity fallback.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BatchProvisionOperation {
    /// Batch entity creation (clause 5.6.7).
    Create,
    /// Batch entity creation or update (clause 5.6.8).
    Upsert(UpsertMode),
    /// Batch entity update (clause 5.6.9), with or without attribute overwrite.
    Update(OverwritePolicy),
    /// Batch entity deletion (clause 5.6.10).
    Delete,
    /// Batch entity merge (clause 5.6.20).
    Merge,
}

impl BatchProvisionOperation {
    /// The clause 4.20 batch operation a registration must advertise to be
    /// forwarded the array in one request.
    const fn batch_operation(self) -> ForwardingOperation {
        match self {
            BatchProvisionOperation::Create => ForwardingOperation::CreateBatch,
            BatchProvisionOperation::Upsert(_) => ForwardingOperation::UpsertBatch,
            BatchProvisionOperation::Update(_) => ForwardingOperation::UpdateBatch,
            BatchProvisionOperation::Delete => ForwardingOperation::DeleteBatch,
            BatchProvisionOperation::Merge => ForwardingOperation::MergeBatch,
        }
    }

    /// The batch endpoint below `/entityOperations` (clause 6.14 to 6.17, 6.31).
    const fn batch_path(self) -> &'static str {
        match self {
            BatchProvisionOperation::Create => "create",
            BatchProvisionOperation::Upsert(_) => "upsert",
            BatchProvisionOperation::Update(_) => "update",
            BatchProvisionOperation::Delete => "delete",
            BatchProvisionOperation::Merge => "merge",
        }
    }
}

/// One entity of the client's array.
pub struct BatchEntity<'a> {
    /// The entity identifier.
    pub id: &'a Uri,
    /// The compact entity type used for registration matching; a deletion
    /// names none.
    pub entity_type: Option<&'a EntityTypeName>,
    /// The entity as the client sent it, without an inline `@context`,
    /// serialised only for a registration it matches; a deletion carries none.
    pub body: Option<&'a dyn ForwardedBody>,
}

/// Parameters for forwarding a batch provision operation.
pub struct ForwardBatchRequest<'a> {
    /// The batch operation being forwarded.
    pub operation: BatchProvisionOperation,
    /// The client's array.
    pub entities: &'a [BatchEntity<'a>],
    /// The JSON-LD context the triggering request resolved to.
    pub context: &'a ContextSpec,
    /// The query string the batch endpoint was called with, forwarded with
    /// every batch request.
    pub query_string: Option<&'a str>,
    /// The request that triggered this forwarding.
    pub triggering: TriggeringRequest<'a>,
}

/// What one registration did with one entity of a forwarded batch.
pub struct BatchForwardedResult {
    /// The registration the entity was forwarded to.
    pub registration_id: Uri,
    /// The entity.
    pub entity_id: Uri,
    /// The registration's forwarding mode.
    pub mode: ForwardingMode,
    /// What the registration answered for the entity.
    pub outcome: ForwardedWriteOutcome,
    /// The attributes of this entity a proxied registration takes out of local
    /// processing (clause 4.3.6.3); empty for an inclusive registration.
    pub proxied_attrs: Vec<AttributeName>,
}

impl FederationService {
    /// Forward a batch provision operation to every matching context source.
    ///
    /// # Errors
    ///
    /// Returns [`FederationError::JsonLd`] when an entity type of the array
    /// cannot be expanded against the request's context,
    /// [`FederationError::Registration`] when the candidate registrations
    /// cannot be read, and [`FederationError::ForwardedBody`] when an entity a
    /// registration matches cannot be serialised for it. A source that rejects part of the array does not fail
    /// the call: its rejections are reported per entity in the returned
    /// [`BatchForwardedResult`]s.
    pub async fn forward_batch(&self, target: &ContextTarget, request: ForwardBatchRequest<'_>) -> Result<Vec<BatchForwardedResult>, FederationError> {
        if !target.permits_federation() {
            return Ok(Vec::new());
        }
        if self.is_loop_detected(request.triggering.via) {
            debug!("Loop detected in Via header chain, skipping batch provision forwarding");
            return Ok(Vec::new());
        }
        let resolver = TermResolver::new(&self.engine, request.context);
        let mut expanded_types: HashMap<&str, IriBuf> = HashMap::new();
        for name in request.entities.iter().filter_map(|entity| entity.entity_type) {
            if !expanded_types.contains_key(name.as_str()) {
                expanded_types.insert(name.as_str(), resolver.expand_type(name.as_str()).await?);
            }
        }
        // Per spec 4.3.6.2: auxiliary CSRs only supplement retrieval, never receive writes.
        let candidates = self
            .csr_repo
            .find_matching(None, None)
            .await?
            .into_iter()
            .filter(|csr| csr.access.mode != Some(ForwardingMode::Auxiliary))
            .collect::<Vec<_>>();
        let candidates = &candidates;
        let request = &request;
        let expanded_types = &expanded_types;
        let resolver = &resolver;
        let results = stream::iter(candidates)
            .map(|csr| async move {
                let narrowed = narrow_input(csr, candidates, request.entities, expanded_types, resolver).await?;
                if narrowed.is_empty() {
                    return Ok::<_, FederationError>(Vec::new());
                }
                Ok(self.drive_registration(csr, narrowed, request).await)
            })
            .buffer_unordered(self.csr_concurrency)
            .collect::<Vec<Result<Vec<BatchForwardedResult>, FederationError>>>()
            .await;
        Ok(results.into_iter().collect::<Result<Vec<_>, _>>()?.into_iter().flatten().collect())
    }

    /// Drive one registration through the clause 5.6.7.4 ladder for its
    /// narrowed array.
    async fn drive_registration(
        &self,
        csr: &ContextSourceRegistration,
        narrowed: Vec<NarrowedEntity>,
        request: &ForwardBatchRequest<'_>,
    ) -> Vec<BatchForwardedResult> {
        let mode = csr.access.mode.unwrap_or_default();
        let result = |entity: &NarrowedEntity, outcome: ForwardedWriteOutcome| BatchForwardedResult {
            registration_id: csr.id.clone(), // clone: every result names the registration it came from
            entity_id: entity.id.clone(),    // clone: every result owns its identifier
            mode,
            outcome,
            proxied_attrs: entity.proxied_attrs.clone(), // clone: the result owns the attributes the registration claims
        };
        match plan_for(request.operation, csr) {
            FallbackPlan::Batch => {
                let ids = narrowed.iter().map(|entity| entity.id.clone()).collect::<Vec<_>>(); // clone: the outcomes are keyed by identifier
                let array = narrowed
                    .iter()
                    .map(|entity| entity.body.clone().unwrap_or_else(|| Value::String(entity.id.to_string()))) // clone: the forwarded array owns its items
                    .collect::<Vec<_>>();
                let body = serde_json::to_vec(&array).ok();
                let url = forwarding_url(
                    csr.access.endpoint.url(),
                    &["entityOperations", request.operation.batch_path()],
                    request.query_string.unwrap_or(""),
                );
                let outcome = self
                    .forward_write(ForwardWrite {
                        registration: csr,
                        url,
                        method: reqwest::Method::POST,
                        body: body.as_deref(),
                        context: request.context,
                        triggering: request.triggering,
                        linked_map: None,
                    })
                    .await;
                let outcomes = batch_outcomes(&ids, &outcome).into_iter().collect::<HashMap<_, _>>();
                narrowed
                    .into_iter()
                    .filter_map(|entity| {
                        let outcome = outcomes.get(&entity.id)?.clone(); // clone: the result owns its outcome
                        Some(result(&entity, outcome))
                    })
                    .collect()
            }
            FallbackPlan::PerEntity(write) => {
                stream::iter(narrowed)
                    .map(|entity| async move {
                        let outcome = self.single_write(write, csr, &entity, request).await;
                        result(&entity, outcome)
                    })
                    .buffer_unordered(self.csr_concurrency)
                    .collect()
                    .await
            }
            FallbackPlan::NotAdvertised => {
                if !is_proxied(csr) {
                    return Vec::new();
                }
                let mode = proxied_mode(csr);
                narrowed
                    .into_iter()
                    .map(|entity| BatchForwardedResult {
                        mode,
                        ..result(&entity, ForwardedWriteOutcome::NotAdvertised)
                    })
                    .collect()
            }
        }
    }

    /// Send one single-entity fallback write, with the clause 5.6.8.4
    /// follow-up when a create is answered `AlreadyExists`.
    async fn single_write(
        &self,
        write: SingleWrite,
        csr: &ContextSourceRegistration,
        entity: &NarrowedEntity,
        request: &ForwardBatchRequest<'_>,
    ) -> ForwardedWriteOutcome {
        let (url, method, body) = single_write_request(write, csr.access.endpoint.url(), entity);
        let outcome = self
            .forward_write(ForwardWrite {
                registration: csr,
                url,
                method,
                body: body.as_deref(),
                context: request.context,
                triggering: request.triggering,
                linked_map: None,
            })
            .await;
        let SingleWrite::CreateThenOnConflict(follow_up) = write else {
            return outcome;
        };
        if !matches!(outcome, ForwardedWriteOutcome::RemoteHttpFailure { status: 409, .. }) {
            return outcome;
        }
        let Some(follow_up) = follow_up_write(follow_up) else {
            return ForwardedWriteOutcome::UpsertFollowUpNotAdvertised;
        };
        let (url, method, body) = single_write_request(follow_up, csr.access.endpoint.url(), entity);
        self.forward_write(ForwardWrite {
            registration: csr,
            url,
            method,
            body: body.as_deref(),
            context: request.context,
            triggering: request.triggering,
            linked_map: None,
        })
        .await
    }
}
