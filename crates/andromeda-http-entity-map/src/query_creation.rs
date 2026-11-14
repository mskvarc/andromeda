//! Handlers creating an `EntityMap` from a query on `/entityMaps` and
//! `/temporal/entityMaps`, by GET and by POST (clauses 5.14.4, 5.14.5, 6.34
//! and 6.35).

use crate::{
    creation::{CreationRendering, MapCreation, create_map},
    error::EntityMapResultExt,
    forwarded_query_string::{ForwardedFilters, body_query_string, forwarded_query_string, forwarded_temporal_query_string, temporal_body_query_string},
    membership_window::membership_window,
    state::{EntityMapRouteServices, EntityMapTenantState},
    validation::{validate_entity_map_query, validate_temporal_entity_map_query},
};
use andromeda_entity_map::fingerprint::{FingerprintedRequest, fingerprint_of};
use andromeda_federation::{entity_map::EntityMapQuery, request::TriggeringRequest, split_entities::SplitEntities};
use andromeda_http_extraction::{
    accept::AcceptFormatNoGeo,
    admitted_temporal_query::{AdmittedTemporalPostQuery, AdmittedTemporalQuery, admitted_body_parameters},
    api_root::ApiRoot,
    body_only_query::BodyOnlyQuery,
    entity_query::EntityQuery,
    jsonld::JsonLd,
    pagination_policy::RequestPagination,
    prefer::PreferHeader,
    request_envelope::NgsiLdRequestEnvelope,
    response_context_request::ResponseContextRequest,
    tenant_state,
};
use andromeda_http_response::problem_response::ProblemResponse;
use andromeda_http_temporal::extension_gate::TemporalExtensionGate;
use andromeda_jsonld::user_context_admission::{SubmittedContext, admit_user_context};
use andromeda_linked_retrieval::admission::LinkedOperation;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_query::{candidate_filters::CandidateFilters, page_window::PageWindow};
use andromeda_ngsi_ld_request::{
    empty::EmptyParams,
    query::{
        entity::parameters::EntityQueryParameters,
        temporal::{TemporalQueryParameters, operation::TemporalQueryOperation},
    },
    query_narrowing::ExecutionScope,
};
use andromeda_temporal::{query_reading::QueryReading, requested_identifiers::RequestedIdentifiers};
use axum::response::Response;
use std::sync::Arc;

/// Whether a creation's local candidates are held to every filter of the
/// request.
///
/// Clauses 5.14.4.4 and 5.14.5.4: under split entities a request that is not
/// limited to the local scope maps every local Entity its selectors match,
/// because a locally held part that fails the other filters may belong to an
/// Entity that meets them once gathered; each request that later runs under
/// the map evaluates them over the merged result.
fn candidate_filters(state: &EntityMapRouteServices, split_entities: Option<bool>, scope: ExecutionScope) -> CandidateFilters {
    match (state.forwarding.split_entities(split_entities), scope) {
        (SplitEntities::Split, ExecutionScope::Distributed) => CandidateFilters::Deferred,
        (SplitEntities::Split, ExecutionScope::Local) | (SplitEntities::Complete, ExecutionScope::Distributed | ExecutionScope::Local) => {
            CandidateFilters::Applied
        }
    }
}

/// Resolve the local membership `window` selects of an entity query (clause
/// 5.14.4.4), held to the filters `filters` says.
async fn local_entity_ids(
    state: &Arc<EntityMapRouteServices>,
    query: &EntityQueryParameters,
    window: &PageWindow,
    filters: CandidateFilters,
    rendering: &CreationRendering<'_>,
) -> Result<Vec<Identifier>, ProblemResponse> {
    let uri = rendering.uri;
    let mut compiled = state
        .entity_query
        .compile_query_public(
            query,
            rendering.context,
            state.entity_query.language_targeting(),
            LinkedOperation::EntityMapQuery,
        )
        .await
        .with_entity_map_instance(uri)?;
    match filters {
        CandidateFilters::Applied => {}
        // Clause 5.14.4.4: the split branch selects on the identifier, the id
        // pattern and the type alone.
        CandidateFilters::Deferred => {
            compiled.selection.without_split_filters();
            compiled.selection.without_attribute_requirement();
        }
    }
    let compiled = compiled;
    state
        .entity_query
        .query_ids(&state.live_target, &compiled, window)
        .await
        .with_entity_map_instance(uri)
}

/// Create the map an entity query selects, over either binding (clause 5.14.4.4).
async fn create_from_entity_query(
    state: &Arc<EntityMapRouteServices>,
    query: &EntityQueryParameters,
    window: &PageWindow,
    forwarded_query: String,
    rendering: &CreationRendering<'_>,
    triggering: TriggeringRequest<'_>,
) -> Result<Response, ProblemResponse> {
    let fingerprint = fingerprint_of(FingerprintedRequest::QueryEntities(query)).with_entity_map_instance(rendering.uri)?;
    let filters = candidate_filters(state, query.split_entities, query.execution_scope());
    let local_ids = local_entity_ids(state, query, window, filters, rendering).await?;
    create_map(
        state,
        MapCreation {
            local_ids,
            lifetime: query.entity_map_lifetime.as_ref(),
            fingerprint,
            query: EntityMapQuery::Entities(query),
            forwarded_query,
            local: query.is_local(),
        },
        rendering,
        triggering,
    )
    .await
}

/// Create the map a temporal query selects, over either binding (clause 5.14.5.4).
async fn create_from_temporal_query(
    state: &Arc<EntityMapRouteServices>,
    params: &TemporalQueryParameters,
    window: &PageWindow,
    forwarded_query: String,
    rendering: &CreationRendering<'_>,
    triggering: TriggeringRequest<'_>,
) -> Result<Response, ProblemResponse> {
    let fingerprint = fingerprint_of(FingerprintedRequest::QueryTemporal(params)).with_entity_map_instance(rendering.uri)?;
    // Table 6.18.3.2-1: the temporal window, `timeproperty` and `lastN` bound
    // the candidates the same way they bound a temporal query.
    let local_ids = state
        .temporal
        // Clause 5.14.5.4: the map being created fixes what this request
        // matched, identifiers included.
        .query_ids(
            &state.live_target,
            params,
            window,
            QueryReading {
                context: rendering.context,
                language_targeting: state.temporal.language_targeting(),
                operation: TemporalQueryOperation::EntityMap.linked(state.temporal.linked_retrieval()),
            },
            RequestedIdentifiers::Narrow,
            candidate_filters(state, params.split_entities, params.execution_scope()),
        )
        .await
        .with_entity_map_instance(rendering.uri)?;
    create_map(
        state,
        MapCreation {
            local_ids,
            lifetime: params.entity_map_lifetime.as_ref(),
            fingerprint,
            query: EntityMapQuery::TemporalEntities(params),
            forwarded_query,
            local: params.is_local(),
        },
        rendering,
        triggering,
    )
    .await
}

/// GET /ngsi-ld/v1/entityMaps: create an entity map from query entities (section 5.14.4, section 6.34.3.1).
pub(crate) async fn create_from_query(
    tenant_state::ExistingTenantState(state): EntityMapTenantState,
    accept: AcceptFormatNoGeo,
    NgsiLdRequestEnvelope { uri, incoming_via, headers }: NgsiLdRequestEnvelope,
    ResponseContextRequest { context, api_root }: ResponseContextRequest,
    prefer: PreferHeader,
    pagination: RequestPagination,
    EntityQuery(query): EntityQuery,
) -> Result<Response, ProblemResponse> {
    let context = admit_user_context(state.entity_query.engine(), SubmittedContext::from(context))
        .await
        .with_entity_map_instance(&uri)?;
    validate_entity_map_query(&query, accept.media_type(), &uri)?;
    let window = membership_window(&pagination, &query.page, &query.ordering, &uri)?;
    let filters = ForwardedFilters::for_split(state.forwarding.split_entities(query.split_entities).is_split());
    let forwarded_query = forwarded_query_string(&uri, filters);
    create_from_entity_query(
        &state,
        &query,
        &window,
        forwarded_query,
        &CreationRendering::builder()
            .context(&context)
            .accept(accept)
            .prefer(prefer)
            .uri(&uri)
            .api_root(&api_root)
            .build(),
        TriggeringRequest {
            via: incoming_via.as_ref(),
            headers: Some(&headers),
        },
    )
    .await
}

/// POST /ngsi-ld/v1/entityMaps: create an entity map from query body (section 5.14.4, section 6.34.3.2).
pub(crate) async fn create_from_query_post(
    tenant_state::ExistingTenantState(state): EntityMapTenantState,
    accept: AcceptFormatNoGeo,
    NgsiLdRequestEnvelope { uri, incoming_via, headers }: NgsiLdRequestEnvelope,
    api_root: ApiRoot,
    prefer: PreferHeader,
    pagination: RequestPagination,
    BodyOnlyQuery(JsonLd { body, context }): BodyOnlyQuery,
) -> Result<Response, ProblemResponse> {
    // Clause 6.3.5: the query is read against the one context the body
    // extractor resolved, admitted first (clause 5.5.7).
    let context = admit_user_context(state.entity_query.engine(), context).await.with_entity_map_instance(&uri)?;
    let body = body.admitted(&context).await?;
    let query = EntityQueryParameters::try_from(body).with_entity_map_instance(&uri)?;
    validate_entity_map_query(&query, accept.media_type(), &uri)?;
    let window = membership_window(&pagination, &query.page, &query.ordering, &uri)?;
    let filters = ForwardedFilters::for_split(state.forwarding.split_entities(query.split_entities).is_split());
    let forwarded_query = body_query_string(&query, filters, &uri);
    create_from_entity_query(
        &state,
        &query,
        &window,
        forwarded_query,
        &CreationRendering::builder()
            .context(&context)
            .accept(accept)
            .prefer(prefer)
            .uri(&uri)
            .api_root(&api_root)
            .build(),
        TriggeringRequest {
            via: incoming_via.as_ref(),
            headers: Some(&headers),
        },
    )
    .await
}

/// GET /ngsi-ld/v1/temporal/entityMaps: create an entity map from temporal query (section 5.14.5, section 6.35.3.1).
pub(crate) async fn create_temporal_from_query(
    tenant_state::ExistingTenantState(state): EntityMapTenantState,
    accept: AcceptFormatNoGeo,
    NgsiLdRequestEnvelope { uri, incoming_via, headers }: NgsiLdRequestEnvelope,
    ResponseContextRequest { context, api_root }: ResponseContextRequest,
    prefer: PreferHeader,
    pagination: RequestPagination,
    AdmittedTemporalQuery { params, .. }: AdmittedTemporalQuery<TemporalExtensionGate>,
) -> Result<Response, ProblemResponse> {
    let context = admit_user_context(state.entity_query.engine(), SubmittedContext::from(context))
        .await
        .with_entity_map_instance(&uri)?;
    validate_temporal_entity_map_query(&params, state.temporal.linked_retrieval(), &uri)?;
    let window = membership_window(&pagination, &params.page, &params.ordering, &uri)?;
    let filters = ForwardedFilters::for_split(state.forwarding.split_entities(params.split_entities).is_split());
    let forwarded_query = forwarded_temporal_query_string(&uri, filters);
    create_from_temporal_query(
        &state,
        &params,
        &window,
        forwarded_query,
        &CreationRendering::builder()
            .context(&context)
            .accept(accept)
            .prefer(prefer)
            .uri(&uri)
            .api_root(&api_root)
            .build(),
        TriggeringRequest {
            via: incoming_via.as_ref(),
            headers: Some(&headers),
        },
    )
    .await
}

/// POST /ngsi-ld/v1/temporal/entityMaps: create an entity map from temporal query body (section 5.14.5, section 6.35.3.2).
pub(crate) async fn create_temporal_from_query_post(
    tenant_state::ExistingTenantState(state): EntityMapTenantState,
    accept: AcceptFormatNoGeo,
    NgsiLdRequestEnvelope { uri, incoming_via, headers }: NgsiLdRequestEnvelope,
    api_root: ApiRoot,
    prefer: PreferHeader,
    pagination: RequestPagination,
    AdmittedTemporalPostQuery { body, url, admission, context }: AdmittedTemporalPostQuery<TemporalExtensionGate, EmptyParams>,
) -> Result<Response, ProblemResponse> {
    // Clause 6.3.5: the query is read against the one context the body
    // extractor resolved, admitted first (clause 5.5.7).
    let context = admit_user_context(state.entity_query.engine(), context).await.with_entity_map_instance(&uri)?;
    let body = body.admitted(&context).await?;
    let params = admitted_body_parameters(body, url, &admission).map_err(|report| ProblemResponse::classified(report, uri.clone()))?;
    validate_temporal_entity_map_query(&params, state.temporal.linked_retrieval(), &uri)?;
    let window = membership_window(&pagination, &params.page, &params.ordering, &uri)?;
    let filters = ForwardedFilters::for_split(state.forwarding.split_entities(params.split_entities).is_split());
    let forwarded_query = temporal_body_query_string(&params, filters);
    create_from_temporal_query(
        &state,
        &params,
        &window,
        forwarded_query,
        &CreationRendering::builder()
            .context(&context)
            .accept(accept)
            .prefer(prefer)
            .uri(&uri)
            .api_root(&api_root)
            .build(),
        TriggeringRequest {
            via: incoming_via.as_ref(),
            headers: Some(&headers),
        },
    )
    .await
}
