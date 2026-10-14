//! Admitting a decoded temporal query before it runs.
//!
//! Admission is everything that can be decided about a query before it reaches
//! the store: folding the extension headers into it, resolving the Entity Map it
//! runs under, confining a non-federating branch to the local scope, validating
//! it, and holding it to the clause 5.7.4.4 ordering rule.

use crate::{
    state::TemporalRouteServices,
    temporal::{ContextSpec, NgsiLdProtocolHeaders, ProblemResponse, ResultInstanceExt, TemporalQueryParameters, query::request_entity_map::resolve_query_map},
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_entity_map::reuse::EntityMapUse;
use andromeda_federation::{
    FederationService,
    forwarded_query::client_query::ClientQuery,
    operation::ForwardingOperation,
    ordering_admission::{OrderedRead, OrderingAdmission},
    request::EntityMapInUse,
};
use andromeda_http_extraction::prefer::PreferHeader;
use andromeda_http_headers::via::Via;
use andromeda_jsonld::user_context_admission::AdmittedContext;
use andromeda_ngsi_ld_request::{error::ParameterError, query::temporal::operation::TemporalQueryOperation};
use axum::http::Uri;

/// One temporal query, decoded and admitted: what the protocol headers
/// contributed to it, and the Entity Map it runs under.
pub(super) struct AdmittedQuery {
    /// The `Prefer` header the response honours (clause 6.3.21).
    pub(super) prefer: PreferHeader,
    /// The `Via` chain the request arrived with, replayed onto every forwarded
    /// copy so a forwarding loop is detected rather than followed.
    pub(super) incoming_via: Option<Via>,
    /// The Entity Map the query runs under (clause 5.5.14).
    pub(super) map_use: EntityMapUse,
}

/// Decode `protocol` into the query, hold the query to every rule that can be
/// decided before it runs, and resolve the Entity Map it runs under.
///
/// The extension headers add to the query before it is validated, so a value a
/// header supplied is held to the same rules as one the query string carried.
/// A branch that cannot federate is confined to the local scope first: clause
/// 5.5.15 makes a Snapshot "implicitly limited to a local scope ... overriding
/// setting a local scope to false".
///
/// # Errors
///
/// The classified failure of an extension header the query cannot take, of
/// resolving the Entity Map, of the clause 5.7.4.4 validation, and of the
/// clause 5.7.4.4 ordering rule.
pub(super) async fn admit_request(
    state: &TemporalRouteServices,
    target: &ContextTarget,
    context: &AdmittedContext,
    protocol: NgsiLdProtocolHeaders,
    params: &mut TemporalQueryParameters,
    instance: &Uri,
) -> Result<AdmittedQuery, ProblemResponse> {
    protocol.extensions().apply_to_temporal_query(params).with_instance(instance)?;
    params.lang = params.lang.take().or(protocol.language_filter);
    let map_use = resolve_query_map(&state.entity_maps, target, params, protocol.entity_map.as_ref(), instance).await?;
    if !target.permits_federation() {
        params.local = Some(true);
    }
    params
        .validate(TemporalQueryOperation::Query, state.temporal.linked_retrieval())
        .with_instance(instance)?;
    refuse_inadmissible_ordering(&state.forwarding, target, context, params, EntityMapInUse::from(&map_use), instance).await?;
    Ok(AdmittedQuery {
        prefer: protocol.prefer,
        incoming_via: protocol.incoming_via,
        map_use,
    })
}

/// Refuse an ordering clause 5.7.4.4 does not admit.
///
/// The bullet raises `BadRequestData` for an `orderBy` on a query "not limited
/// to the local scope", which `andromeda_federation::ordering_admission` reads
/// as an answer a registered Context Source takes part in assembling. That is a
/// question for the Context Registry rather than for the parameters, so it is
/// asked here rather than during validation. Which member may order a temporal
/// query is a separate rule the parameters do settle, and validation keeps it.
///
/// # Errors
///
/// The classified failure of reading the registrations that would take part,
/// and the clause 5.7.4.4 refusal itself when one of them does.
async fn refuse_inadmissible_ordering(
    forwarding: &FederationService,
    target: &ContextTarget,
    context: &ContextSpec,
    params: &TemporalQueryParameters,
    entity_map: EntityMapInUse<'_>,
    instance: &Uri,
) -> Result<(), ProblemResponse> {
    let admission = forwarding
        .admit_ordering(
            target,
            context,
            OrderedRead {
                ordering: &params.ordering,
                requested_scope: params.execution_scope(),
                client_query: ClientQuery::Temporal(params),
                operation: ForwardingOperation::QueryTemporalEntity,
                entity_map,
            },
        )
        .await
        .with_instance(instance)?;
    match admission {
        OrderingAdmission::Admitted => Ok(()),
        OrderingAdmission::Refused => Err(ParameterError::OrderingWithParticipatingSource).with_instance(instance),
    }
}
