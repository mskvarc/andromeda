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

use crate::{
    FederationService,
    error::FederationError,
    forwarding_url::forwarding_url,
    operation::ForwardingOperation,
    outcome::ForwardedWriteOutcome,
    provision::{is_proxied, proxied_mode},
    registration_selection::{covered_attribute_members, csr_has_matching_information, is_entity_control_member, retain_covered_attributes},
    request::TriggeringRequest,
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
use url::Url;

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
    /// The entity as the client sent it, without an inline `@context`; a
    /// deletion carries none.
    pub body: Option<&'a Value>,
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

/// How one registration is driven for a batch operation it may not advertise.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FallbackPlan {
    /// The batch operation is advertised: one request carries the array.
    Batch,
    /// Only the single-entity operation is advertised: one request per entity.
    PerEntity(SingleWrite),
    /// Neither is advertised.
    NotAdvertised,
}

/// The single-entity write a batch falls back to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SingleWrite {
    /// Create Entity (clause 5.6.1).
    Create,
    /// Create Entity, followed up when the source reports the entity exists
    /// (clause 5.6.8.4).
    CreateThenOnConflict(UpsertFollowUp),
    /// Replace Entity (clause 5.6.18).
    Replace,
    /// Update Attributes (clause 5.6.2).
    UpdateAttributes,
    /// Append Attributes with overwrite disabled (clause 5.6.3).
    AppendAttributesNoOverwrite,
    /// Delete Entity (clause 5.6.6).
    Delete,
    /// Merge Entity (clause 5.6.17).
    Merge,
}

/// What a batch upsert does after a source reports `AlreadyExists` on the
/// per-entity create (clause 5.6.8.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UpsertFollowUp {
    /// Replace Entity, the mode being replace and the operation advertised.
    Replace,
    /// Update Attributes, the mode being update and the operation advertised.
    UpdateAttributes,
    /// Nothing the mode calls for is advertised: `OperationNotSupported`.
    NotAdvertised,
}

/// The clause 5.6.7.4 to 5.6.20.4 decision ladder for one registration.
pub(crate) fn plan_for(operation: BatchProvisionOperation, csr: &ContextSourceRegistration) -> FallbackPlan {
    if operation.batch_operation().matches(csr) {
        return FallbackPlan::Batch;
    }
    let advertised = |single: ForwardingOperation| single.matches(csr);
    match operation {
        BatchProvisionOperation::Create if advertised(ForwardingOperation::CreateEntity) => FallbackPlan::PerEntity(SingleWrite::Create),
        BatchProvisionOperation::Upsert(mode) if advertised(ForwardingOperation::CreateEntity) => {
            let follow_up = match mode {
                UpsertMode::Replace if advertised(ForwardingOperation::ReplaceEntity) => UpsertFollowUp::Replace,
                UpsertMode::Update if advertised(ForwardingOperation::UpdateEntityAttributes) => UpsertFollowUp::UpdateAttributes,
                UpsertMode::Replace | UpsertMode::Update => UpsertFollowUp::NotAdvertised,
            };
            FallbackPlan::PerEntity(SingleWrite::CreateThenOnConflict(follow_up))
        }
        BatchProvisionOperation::Upsert(UpsertMode::Replace) if advertised(ForwardingOperation::ReplaceEntity) => FallbackPlan::PerEntity(SingleWrite::Replace),
        BatchProvisionOperation::Upsert(UpsertMode::Update) | BatchProvisionOperation::Update(OverwritePolicy::Overwrite)
            if advertised(ForwardingOperation::UpdateEntityAttributes) =>
        {
            FallbackPlan::PerEntity(SingleWrite::UpdateAttributes)
        }
        BatchProvisionOperation::Update(OverwritePolicy::Preserve) if advertised(ForwardingOperation::AppendEntityAttributes) => {
            FallbackPlan::PerEntity(SingleWrite::AppendAttributesNoOverwrite)
        }
        BatchProvisionOperation::Delete if advertised(ForwardingOperation::DeleteEntity) => FallbackPlan::PerEntity(SingleWrite::Delete),
        BatchProvisionOperation::Merge if advertised(ForwardingOperation::MergeEntity) => FallbackPlan::PerEntity(SingleWrite::Merge),
        BatchProvisionOperation::Create
        | BatchProvisionOperation::Upsert(_)
        | BatchProvisionOperation::Update(_)
        | BatchProvisionOperation::Delete
        | BatchProvisionOperation::Merge => FallbackPlan::NotAdvertised,
    }
}

/// One entity of the array narrowed for one registration.
struct NarrowedEntity {
    /// The entity identifier.
    id: Uri,
    /// The narrowed entity; none for a deletion.
    body: Option<Value>,
    /// The attributes a proxied registration claims from it.
    proxied_attrs: Vec<AttributeName>,
}

/// Narrow the client's array to what `csr` receives (the `IN` of clauses
/// 5.6.7.4 and following): the entities it matches, each reduced to the
/// attributes it covers minus those another exclusive registration -- or,
/// unless `csr` is itself a redirect, another redirect registration -- owns,
/// and without the entities that keeps no attribute.
fn narrow_input(
    csr: &ContextSourceRegistration,
    candidates: &[ContextSourceRegistration],
    entities: &[BatchEntity<'_>],
    expanded_types: &HashMap<&str, IriBuf>,
) -> Vec<NarrowedEntity> {
    let csr_is_redirect = csr.access.mode == Some(ForwardingMode::Redirect);
    entities
        .iter()
        .filter_map(|entity| {
            let entity_type = entity.entity_type.and_then(|name| expanded_types.get(name.as_str()));
            if !csr_has_matching_information(csr, entity_type, Some(entity.id), None) {
                return None;
            }
            let Some(body) = entity.body else {
                return Some(NarrowedEntity {
                    id: entity.id.clone(), // clone: the narrowed entity outlives the borrowed array
                    body: None,
                    proxied_attrs: Vec::new(),
                });
            };
            let mut object = body.as_object()?.clone(); // clone: each registration gets its own narrowed copy of the entity
            if !retain_covered_attributes(&mut object, csr, entity_type, Some(entity.id)) {
                return None;
            }
            for other in candidates.iter().filter(|other| other.id != csr.id) {
                let owns = match other.access.mode {
                    Some(ForwardingMode::Exclusive) => true,
                    Some(ForwardingMode::Redirect) => !csr_is_redirect,
                    Some(ForwardingMode::Inclusive | ForwardingMode::Auxiliary) | None => false,
                };
                if !owns {
                    continue;
                }
                for member in covered_attribute_members(&object, other, entity_type, Some(entity.id)) {
                    object.remove(&member);
                }
            }
            let remaining = object
                .keys()
                .filter(|name| !is_entity_control_member(name))
                .filter_map(|name| name.parse::<AttributeName>().ok())
                .collect::<Vec<_>>();
            if remaining.is_empty() {
                return None;
            }
            Some(NarrowedEntity {
                id: entity.id.clone(), // clone: the narrowed entity outlives the borrowed array
                body: Some(Value::Object(object)),
                proxied_attrs: if is_proxied(csr) { remaining } else { Vec::new() },
            })
        })
        .collect()
}

/// The per-entity outcomes a forwarded batch request answered with
/// (clauses 5.6.7.5 and following): a clause 5.2.16 body is read entity by
/// entity, an array of identifiers names the successes, and a bodiless
/// success or any failure applies to every entity of the array.
fn batch_outcomes(ids: &[Uri], outcome: &ForwardedWriteOutcome) -> Vec<(Uri, ForwardedWriteOutcome)> {
    let ForwardedWriteOutcome::Completed { status, body: Some(body) } = outcome else {
        return ids
            .iter()
            .map(|id| (id.clone(), outcome.clone())) // clone: every entity owns its identifier and the outcome it shares
            .collect();
    };
    let listed_errors = body
        .get("errors")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|error| {
            let id = error.get("entityId")?.as_str()?.parse::<Uri>().ok()?;
            let problem = error.get("error")?;
            let status = problem
                .get("status")
                .and_then(Value::as_u64)
                .and_then(|status| u16::try_from(status).ok())
                .unwrap_or(500);
            Some((
                id,
                ForwardedWriteOutcome::RemoteHttpFailure {
                    status,
                    body: Some(problem.clone()), // clone: the per-entity outcome owns the problem detail reported for it
                },
            ))
        })
        .collect::<HashMap<_, _>>();
    ids.iter()
        .map(|id| {
            let outcome = listed_errors
                .get(id)
                .cloned()
                .unwrap_or(ForwardedWriteOutcome::Completed { status: *status, body: None }); // clone: the result owns its outcome
            (id.clone(), outcome) // clone: the result owns its identifier
        })
        .collect()
}

/// The request one single-entity fallback write sends for `entity`.
fn single_write_request(write: SingleWrite, endpoint: &Url, entity: &NarrowedEntity) -> (Result<Url, FederationError>, reqwest::Method, Option<Vec<u8>>) {
    let id = entity.id.as_str();
    let fragment = || entity.body.as_ref().map(without_identifier).and_then(|body| serde_json::to_vec(&body).ok());
    let whole = || entity.body.as_ref().and_then(|body| serde_json::to_vec(body).ok());
    match write {
        SingleWrite::Create | SingleWrite::CreateThenOnConflict(_) => (forwarding_url(endpoint, &["entities"], ""), reqwest::Method::POST, whole()),
        SingleWrite::Replace => (forwarding_url(endpoint, &["entities", id], ""), reqwest::Method::PUT, whole()),
        SingleWrite::UpdateAttributes => (forwarding_url(endpoint, &["entities", id, "attrs"], ""), reqwest::Method::PATCH, fragment()),
        SingleWrite::AppendAttributesNoOverwrite => (
            forwarding_url(endpoint, &["entities", id, "attrs"], "options=noOverwrite"),
            reqwest::Method::POST,
            fragment(),
        ),
        SingleWrite::Delete => (forwarding_url(endpoint, &["entities", id], ""), reqwest::Method::DELETE, None),
        SingleWrite::Merge => (forwarding_url(endpoint, &["entities", id], ""), reqwest::Method::PATCH, fragment()),
    }
}

/// An entity fragment for the attribute-level operations: the entity without
/// its `id`, which the request path already names.
fn without_identifier(body: &Value) -> Value {
    match body.as_object() {
        Some(object) => Value::Object(
            object
                .iter()
                .filter(|(name, _)| name.as_str() != "id")
                .map(|(name, value)| (name.clone(), value.clone()))
                .collect(),
        ), // clone: the fragment is a new value
        None => body.clone(), // clone: same
    }
}

/// The clause 5.6.8.4 follow-up of a per-entity create answered `AlreadyExists`.
const fn follow_up_write(follow_up: UpsertFollowUp) -> Option<SingleWrite> {
    match follow_up {
        UpsertFollowUp::Replace => Some(SingleWrite::Replace),
        UpsertFollowUp::UpdateAttributes => Some(SingleWrite::UpdateAttributes),
        UpsertFollowUp::NotAdvertised => None,
    }
}

impl FederationService {
    /// Forward a batch provision operation to every matching context source.
    ///
    /// # Errors
    ///
    /// Returns [`FederationError::JsonLd`] when an entity type of the array
    /// cannot be expanded against the request's context, and
    /// [`FederationError::Registration`] when the candidate registrations
    /// cannot be read. A source that rejects part of the array does not fail
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
        let prepared_context = self.engine.prepare_term_context(request.context);
        let mut expanded_types: HashMap<&str, IriBuf> = HashMap::new();
        for name in request.entities.iter().filter_map(|entity| entity.entity_type) {
            if !expanded_types.contains_key(name.as_str()) {
                expanded_types.insert(name.as_str(), self.engine.expand_type(name.as_str(), &prepared_context).await?);
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
        let results = stream::iter(candidates)
            .map(|csr| async move {
                let narrowed = narrow_input(csr, candidates, request.entities, expanded_types);
                if narrowed.is_empty() {
                    return Vec::new();
                }
                self.drive_registration(csr, narrowed, request).await
            })
            .buffer_unordered(self.csr_concurrency)
            .collect::<Vec<_>>()
            .await;
        Ok(results.into_iter().flatten().collect())
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
                    .forward_write(csr, url, reqwest::Method::POST, body.as_deref(), request.context, request.triggering)
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
        let outcome = self.forward_write(csr, url, method, body.as_deref(), request.context, request.triggering).await;
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
        self.forward_write(csr, url, method, body.as_deref(), request.context, request.triggering).await
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        outcome::{ForwardedWriteOutcome, TransportFailure},
        provision_batch::{
            BatchEntity,
            BatchProvisionOperation,
            FallbackPlan,
            NarrowedEntity,
            SingleWrite,
            UpsertFollowUp,
            batch_outcomes,
            narrow_input,
            plan_for,
            single_write_request,
        },
    };
    use andromeda_ngsi_ld_lexicon::{entity_type_name::EntityTypeName, uri::Uri};
    use andromeda_ngsi_ld_model::{expanded::mutation::selection::OverwritePolicy, registration::ContextSourceRegistration};
    use andromeda_ngsi_ld_request::options::UpsertMode;
    use iri_rs::IriBuf;
    use serde_json::{Value, json};
    use std::{collections::HashMap, slice::from_ref};
    use url::Url;

    fn uri(raw: &str) -> Uri {
        raw.parse().expect("valid URI")
    }

    fn registration(
        id: &str,
        mode: &str,
        entity_type: Option<&str>,
        entity_id: Option<&str>,
        properties: &[&str],
        operations: &[&str],
    ) -> ContextSourceRegistration {
        let mut selector = json!({});
        if let Some(entity_type) = entity_type {
            selector["type"] = json!(entity_type);
        }
        if let Some(entity_id) = entity_id {
            selector["id"] = json!(entity_id);
        }
        let mut information = json!({});
        if entity_type.is_some() || entity_id.is_some() {
            information["entities"] = json!([selector]);
        }
        if !properties.is_empty() {
            information["propertyNames"] = json!(properties);
        }
        serde_json::from_value(json!({
            "id": id,
            "type": "ContextSourceRegistration",
            "endpoint": "https://peer.example.org",
            "mode": mode,
            "information": [information],
            "operations": operations,
            "status": "ok",
            "createdAt": "2024-01-01T00:00:00Z",
            "modifiedAt": "2024-01-01T00:00:00Z"
        }))
        .expect("valid registration")
    }

    #[test]
    fn the_fallback_ladder_follows_the_clause_5_6_7_4_to_5_6_20_4_decision_table() {
        let cases: Vec<(BatchProvisionOperation, &[&str], FallbackPlan)> = vec![
            (BatchProvisionOperation::Create, &["createBatch"], FallbackPlan::Batch),
            (BatchProvisionOperation::Create, &["createEntity"], FallbackPlan::PerEntity(SingleWrite::Create)),
            (BatchProvisionOperation::Create, &["retrieveEntity"], FallbackPlan::NotAdvertised),
            (BatchProvisionOperation::Upsert(UpsertMode::Replace), &["upsertBatch"], FallbackPlan::Batch),
            (
                BatchProvisionOperation::Upsert(UpsertMode::Replace),
                &["createEntity", "replaceEntity"],
                FallbackPlan::PerEntity(SingleWrite::CreateThenOnConflict(UpsertFollowUp::Replace)),
            ),
            (
                BatchProvisionOperation::Upsert(UpsertMode::Update),
                &["createEntity", "updateEntity"],
                FallbackPlan::PerEntity(SingleWrite::CreateThenOnConflict(UpsertFollowUp::UpdateAttributes)),
            ),
            (
                BatchProvisionOperation::Upsert(UpsertMode::Replace),
                &["createEntity", "updateEntity"],
                FallbackPlan::PerEntity(SingleWrite::CreateThenOnConflict(UpsertFollowUp::NotAdvertised)),
            ),
            (
                BatchProvisionOperation::Upsert(UpsertMode::Replace),
                &["replaceEntity"],
                FallbackPlan::PerEntity(SingleWrite::Replace),
            ),
            (
                BatchProvisionOperation::Upsert(UpsertMode::Update),
                &["updateEntity"],
                FallbackPlan::PerEntity(SingleWrite::UpdateAttributes),
            ),
            (
                BatchProvisionOperation::Upsert(UpsertMode::Update),
                &["replaceEntity"],
                FallbackPlan::NotAdvertised,
            ),
            (
                BatchProvisionOperation::Update(OverwritePolicy::Overwrite),
                &["updateBatch"],
                FallbackPlan::Batch,
            ),
            (
                BatchProvisionOperation::Update(OverwritePolicy::Overwrite),
                &["updateEntity"],
                FallbackPlan::PerEntity(SingleWrite::UpdateAttributes),
            ),
            (
                BatchProvisionOperation::Update(OverwritePolicy::Overwrite),
                &["appendAttrs"],
                FallbackPlan::NotAdvertised,
            ),
            (
                BatchProvisionOperation::Update(OverwritePolicy::Preserve),
                &["appendAttrs"],
                FallbackPlan::PerEntity(SingleWrite::AppendAttributesNoOverwrite),
            ),
            (
                BatchProvisionOperation::Update(OverwritePolicy::Preserve),
                &["updateEntity"],
                FallbackPlan::NotAdvertised,
            ),
            (BatchProvisionOperation::Delete, &["deleteBatch"], FallbackPlan::Batch),
            (BatchProvisionOperation::Delete, &["deleteEntity"], FallbackPlan::PerEntity(SingleWrite::Delete)),
            (
                BatchProvisionOperation::Delete,
                &["redirectionOps"],
                FallbackPlan::PerEntity(SingleWrite::Delete),
            ),
            (BatchProvisionOperation::Merge, &["mergeBatch"], FallbackPlan::Batch),
            (BatchProvisionOperation::Merge, &["mergeEntity"], FallbackPlan::PerEntity(SingleWrite::Merge)),
            (BatchProvisionOperation::Merge, &["retrieveEntity"], FallbackPlan::NotAdvertised),
        ];
        for (operation, operations, expected) in cases {
            let csr = registration("urn:ngsi-ld:csr:1", "redirect", Some("https://example.org/Vehicle"), None, &[], operations);
            pretty_assertions::assert_eq!(plan_for(operation, &csr), expected, "{operation:?} with {operations:?}");
        }
    }

    fn vehicle(id: &str) -> Value {
        json!({"id": id, "type": "Vehicle", "brandName": {"type": "Property", "value": "x"}, "speed": {"type": "Property", "value": 1}, "location": {"type": "Property", "value": "here"}})
    }

    #[test]
    fn each_registration_receives_its_own_narrowed_copy_and_loses_what_other_proxied_sources_own() {
        // Clause 5.6.7.4: per registration, unmatched entities are dropped,
        // non-matching attributes are removed, attributes another exclusive or
        // redirect registration owns are removed, and an entity left without
        // attributes is dropped.
        let speed = registration(
            "urn:ngsi-ld:csr:speed",
            "redirect",
            Some("https://example.org/Vehicle"),
            None,
            &["https://example.org/speed"],
            &["createBatch"],
        );
        let location = registration(
            "urn:ngsi-ld:csr:location",
            "redirect",
            Some("https://example.org/Vehicle"),
            None,
            &["https://example.org/location"],
            &["createBatch"],
        );
        let everything = registration(
            "urn:ngsi-ld:csr:all",
            "inclusive",
            Some("https://example.org/Vehicle"),
            None,
            &[],
            &["createBatch"],
        );
        let candidates = vec![speed.clone(), location, everything.clone()]; // clone: the candidate list and the subjects both own the registrations
        let v1 = uri("urn:ngsi-ld:Vehicle:1");
        let d1 = uri("urn:ngsi-ld:Device:1");
        let vehicle_type = "Vehicle".parse::<EntityTypeName>().expect("valid type");
        let device_type = "Device".parse::<EntityTypeName>().expect("valid type");
        let vehicle_body = vehicle("urn:ngsi-ld:Vehicle:1");
        let device_body = json!({"id": "urn:ngsi-ld:Device:1", "type": "Device", "brandName": {"type": "Property", "value": "sensor"}});
        let entities = [
            BatchEntity {
                id: &v1,
                entity_type: Some(&vehicle_type),
                body: Some(&vehicle_body),
            },
            BatchEntity {
                id: &d1,
                entity_type: Some(&device_type),
                body: Some(&device_body),
            },
        ];
        let expanded_types = HashMap::from([
            ("Vehicle", IriBuf::new("https://example.org/Vehicle".to_string()).expect("valid IRI")),
            ("Device", IriBuf::new("https://example.org/Device".to_string()).expect("valid IRI")),
        ]);

        let for_speed = narrow_input(&speed, &candidates, &entities, &expanded_types);
        pretty_assertions::assert_eq!(for_speed.len(), 1, "the Device is not matched");
        let body = for_speed[0].body.as_ref().expect("a narrowed body");
        assert!(
            body.get("speed").is_some() && body.get("location").is_none() && body.get("brandName").is_none(),
            "{body}"
        );
        pretty_assertions::assert_eq!(for_speed[0].proxied_attrs.iter().map(ToString::to_string).collect::<Vec<_>>(), vec!["speed"]);

        let for_everything = narrow_input(&everything, &candidates, &entities, &expanded_types);
        let body = for_everything[0].body.as_ref().expect("a narrowed body");
        assert!(body.get("brandName").is_some(), "the unowned attribute stays: {body}");
        assert!(
            body.get("speed").is_none() && body.get("location").is_none(),
            "the redirect-owned attributes leave the inclusive copy: {body}"
        );
        assert!(for_everything[0].proxied_attrs.is_empty(), "an inclusive registration proxies nothing");

        let colour = registration(
            "urn:ngsi-ld:csr:colour",
            "redirect",
            Some("https://example.org/Vehicle"),
            None,
            &["https://example.org/colour"],
            &["createBatch"],
        );
        assert!(
            narrow_input(&colour, &candidates, &entities, &expanded_types).is_empty(),
            "a source owning nothing present is skipped"
        );
    }

    #[test]
    fn a_deletion_narrows_by_identifier_alone() {
        // A deletion names no type, so the registration's type selector is
        // not consulted and only its identifier narrows the array.
        let owner = registration(
            "urn:ngsi-ld:csr:v1",
            "redirect",
            Some("https://example.org/Vehicle"),
            Some("urn:ngsi-ld:Vehicle:1"),
            &[],
            &["deleteBatch"],
        );
        let v1 = uri("urn:ngsi-ld:Vehicle:1");
        let v2 = uri("urn:ngsi-ld:Vehicle:2");
        let entities = [
            BatchEntity {
                id: &v1,
                entity_type: None,
                body: None,
            },
            BatchEntity {
                id: &v2,
                entity_type: None,
                body: None,
            },
        ];
        let narrowed = narrow_input(&owner, from_ref(&owner), &entities, &HashMap::new());
        pretty_assertions::assert_eq!(
            narrowed.iter().map(|entity| entity.id.to_string()).collect::<Vec<_>>(),
            vec!["urn:ngsi-ld:Vehicle:1"]
        );
    }

    #[test]
    fn a_forwarded_batch_answer_is_read_entity_by_entity() {
        let ids = [uri("urn:ngsi-ld:Vehicle:1"), uri("urn:ngsi-ld:Vehicle:2")];
        let multi_status = ForwardedWriteOutcome::Completed {
            status: 207,
            body: Some(json!({
                "success": ["urn:ngsi-ld:Vehicle:2"],
                "errors": [{"entityId": "urn:ngsi-ld:Vehicle:1", "error": {"status": 409, "type": "https://uri.etsi.org/ngsi-ld/errors/AlreadyExists", "title": "exists"}}]
            })),
        };
        let outcomes = batch_outcomes(&ids, &multi_status).into_iter().collect::<HashMap<_, _>>();
        assert!(matches!(outcomes[&ids[0]], ForwardedWriteOutcome::RemoteHttpFailure { status: 409, .. }));
        assert!(matches!(outcomes[&ids[1]], ForwardedWriteOutcome::Completed { status: 207, .. }));

        let bodiless = batch_outcomes(&ids, &ForwardedWriteOutcome::Completed { status: 204, body: None });
        assert!(
            bodiless
                .iter()
                .all(|(_, outcome)| matches!(outcome, ForwardedWriteOutcome::Completed { status: 204, .. }))
        );

        let failed = batch_outcomes(
            &ids,
            &ForwardedWriteOutcome::Transport(TransportFailure::Timeout {
                detail: "no response".to_string(),
            }),
        );
        pretty_assertions::assert_eq!(failed.len(), 2);
        assert!(failed.iter().all(|(_, outcome)| matches!(outcome, ForwardedWriteOutcome::Transport(_))));
    }

    #[test]
    fn every_single_entity_fallback_addresses_its_clause_6_4_and_6_5_resource() {
        let endpoint = Url::parse("http://peer").expect("valid URL");
        let entity = NarrowedEntity {
            id: uri("urn:ngsi-ld:Vehicle:1"),
            body: Some(json!({"id": "urn:ngsi-ld:Vehicle:1", "type": "Vehicle", "speed": {"type": "Property", "value": 1}})),
            proxied_attrs: Vec::new(),
        };
        let cases = [
            (SingleWrite::Create, reqwest::Method::POST, "http://peer/ngsi-ld/v1/entities", true),
            (
                SingleWrite::CreateThenOnConflict(UpsertFollowUp::Replace),
                reqwest::Method::POST,
                "http://peer/ngsi-ld/v1/entities",
                true,
            ),
            (
                SingleWrite::Replace,
                reqwest::Method::PUT,
                "http://peer/ngsi-ld/v1/entities/urn:ngsi-ld:Vehicle:1",
                true,
            ),
            (
                SingleWrite::UpdateAttributes,
                reqwest::Method::PATCH,
                "http://peer/ngsi-ld/v1/entities/urn:ngsi-ld:Vehicle:1/attrs",
                false,
            ),
            (
                SingleWrite::AppendAttributesNoOverwrite,
                reqwest::Method::POST,
                "http://peer/ngsi-ld/v1/entities/urn:ngsi-ld:Vehicle:1/attrs?options=noOverwrite",
                false,
            ),
            (
                SingleWrite::Delete,
                reqwest::Method::DELETE,
                "http://peer/ngsi-ld/v1/entities/urn:ngsi-ld:Vehicle:1",
                false,
            ),
            (
                SingleWrite::Merge,
                reqwest::Method::PATCH,
                "http://peer/ngsi-ld/v1/entities/urn:ngsi-ld:Vehicle:1",
                false,
            ),
        ];
        for (write, expected_method, expected_url, keeps_id) in cases {
            let (url, method, body) = single_write_request(write, &endpoint, &entity);
            pretty_assertions::assert_eq!(url.expect("joins").as_str(), expected_url, "{write:?}");
            pretty_assertions::assert_eq!(method, expected_method, "{write:?}");
            if write == SingleWrite::Delete {
                assert!(body.is_none(), "a deletion carries no body");
                continue;
            }
            let body: Value = serde_json::from_slice(&body.expect("a body")).expect("JSON body");
            pretty_assertions::assert_eq!(body.get("id").is_some(), keeps_id, "{write:?}");
            assert!(body.get("speed").is_some(), "{write:?}");
        }
    }
}
