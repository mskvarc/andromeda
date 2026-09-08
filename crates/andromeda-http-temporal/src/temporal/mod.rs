//! Typed handlers for the NGSI-LD Temporal API.

use crate::{
    error::{TemporalResultExt as ResultInstanceExt, classify_temporal_error},
    state::{TemporalTenantState as TenantState, TemporalTenantStateOrCreate as TenantStateOrCreate},
};
use andromeda_federation::{
    entity_result::{collect_proxied_attrs, collect_temporal_warnings, strip_attributes_from_value},
    operation::{ForwardingOperation, ProvisionResource},
    outcome::ForwardedWriteResult,
    request::{CsrModeFilter, ForwardProvisionRequest, ForwardTemporalQueryRequest, TriggeringRequest},
    temporal_result::merge_temporal_retrieve_results,
};
use andromeda_http_extraction::{
    accept::AcceptFormatNoGeo,
    context::ContextUrl,
    jsonld::JsonLd,
    prefer::PreferHeader,
    protocol::NgsiLdProtocolHeaders,
    query::ValidatedQuery,
    typed_path::TypedPath,
    via::IncomingVia,
};
use andromeda_http_response::{
    collection::NgsiLdCollectionResponse,
    problem_response::ProblemResponse,
    provision::{LocalOutcome, provision_response},
    render::start_rendering,
    single::NgsiLdResponse,
};
use andromeda_jsonld::{context_spec::ContextSpec, term_expansion::ContextualExpander};
use andromeda_ngsi_ld_lexicon::{
    attribute_name::AttributeName,
    entity_type_name::EntityTypeName,
    instance_family::InstanceFamily,
    instance_identity::InstanceIdentity,
    time_rel::TimeRel,
    uri::Uri,
};
use andromeda_ngsi_ld_model::{
    expanded::query_match::{matches_geo_query, matches_scope_query},
    registration::ForwardingMode,
};
use andromeda_ngsi_ld_query::{
    filter::{eval::evaluate, expansion::expand_q_expr},
    geo::compile::compile_geo_query,
};
use andromeda_ngsi_ld_render::{
    response::ResponsePayload,
    temporal::simplify::{TemporalSimplifyPayload, TemporalSimplifyRenderer},
};
use andromeda_ngsi_ld_representation::entity_members::ENTITY_MEMBERS;
use andromeda_ngsi_ld_request::{
    attribute_deletion::DeleteAttrParams,
    deferred_split_filters::DeferredSplitFilters,
    local::LocalOnlyParams,
    query::temporal::TemporalQueryParameters,
};
use andromeda_temporal::{
    error::TemporalError,
    port::mutator::TemporalDeleteScope as DeleteScope,
    service::{CreateOrUpdateResult, TemporalQueryResult},
};
use axum::{
    extract::OriginalUri,
    http::{HeaderMap, Method, StatusCode},
    response::{IntoResponse, Response},
};
use serde_json::Value;

/// Whether a temporal payload still carries an attribute once the proxied
/// ones are gone (clause 5.6.11.4: "the remaining input data").
fn has_attributes(payload: &Value) -> bool {
    payload
        .as_object()
        .is_some_and(|members| members.keys().any(|member| !ENTITY_MEMBERS.contains(&member.as_str())))
}

/// The entity type a temporal payload names, for registration matching: its
/// single type or the first of several.
fn payload_entity_type(payload: &Value) -> Option<EntityTypeName> {
    let type_member = payload.get("type")?;
    let first = type_member.as_str().or_else(|| type_member.as_array()?.first()?.as_str())?;
    first.parse().ok()
}

/// The local outcome of a temporal write on an entity registrations also
/// hold: clauses 5.6.12.4 to 5.6.16.4 raise `ResourceNotFound` only when the
/// entity is held neither locally nor by a matching registration, so a local
/// not-found beside a matching registration is nothing left to do here.
fn local_temporal_outcome<T>(result: Result<T, TemporalError>, forwarded: &[ForwardedWriteResult]) -> LocalOutcome {
    match result {
        Err(TemporalError::NotFound { .. }) if !forwarded.is_empty() => LocalOutcome::NothingRemained,
        other => LocalOutcome::processed(other, classify_temporal_error),
    }
}

/// Temporal add attributes handling.
pub mod add_attributes;
/// Handles `POST /ngsi-ld/v1/temporal/entities`.
///
/// Creates a new temporal entity or updates an existing one from the request payload.
/// Temporal create or update handling.
pub mod create_or_update;
/// Temporal delete handling.
pub mod delete;
/// Temporal delete attribute handling.
pub mod delete_attribute;
/// Temporal delete attribute instance handling.
pub mod delete_attribute_instance;
/// Temporal query handling.
pub mod query;
/// Temporal retrieve handling.
pub mod retrieve;
/// Temporal update attribute instance handling.
pub mod update_attribute_instance;
