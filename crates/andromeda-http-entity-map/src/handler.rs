//! Handlers for the `/entityMaps` and `/temporal/entityMaps` endpoints (clauses 5.14, 6.34, 6.35).

use crate::{
    creation::{CreationRendering, MapCreation, create_map},
    error::{EntityMapResultExt, bad_request_data, internal_problem},
    forwarded_query_string::{ForwardedFilters, body_query_string, forwarded_query_string, forwarded_temporal_query_string, temporal_body_query_string},
    membership_window::membership_window,
    state::{EntityMapRouteServices, EntityMapTenantState},
    validation::{validate_entity_map_query, validate_temporal_entity_map_query},
};
use andromeda_entity_map::fingerprint::{FingerprintedRequest, fingerprint_of};
use andromeda_federation::{entity_map::EntityMapQueryKind, request::TriggeringRequest};
use andromeda_http_context_registry::response_rendering::ResponseContextSource;
use andromeda_http_extraction::{
    accept::AcceptFormatNoGeo,
    api_root::ApiRoot,
    entity_query::EntityQuery,
    jsonld::JsonLd,
    pagination_policy::RequestPagination,
    prefer::PreferHeader,
    query::ValidatedQuery,
    request_envelope::NgsiLdRequestEnvelope,
    response_context_request::ResponseContextRequest,
    temporal_query::TemporalQuery,
    tenant_state,
    typed_path::TypedPath,
};
use andromeda_http_response::{internal_failure_summary::InternalFailureSummary, problem_response::ProblemResponse, single::NgsiLdResponse};
use andromeda_jsonld::{
    context_spec::ContextSpec,
    user_context_admission::{SubmittedContext, admit_user_context},
};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::query::body::Query;
use andromeda_ngsi_ld_query::page_window::PageWindow;
use andromeda_ngsi_ld_render::{entity_map_body::EntityMapBody, response::ResponsePayload};
use andromeda_ngsi_ld_request::{
    body::entity_map::UpdateEntityMapBody,
    empty::EmptyParams,
    query::{entity::parameters::EntityQueryParameters, temporal::TemporalQueryParameters},
};
use andromeda_temporal::requested_identifiers::RequestedIdentifiers;
use axum::{
    extract::OriginalUri,
    http::{StatusCode, Uri as HttpUri},
    response::{IntoResponse, Response},
};
use std::sync::Arc;

/// GET /ngsi-ld/v1/entityMaps/{entityMapId} -- Retrieve an entity map (section 5.14.1).
pub async fn get(
    // Clause 6.3.20: the operation defines no parameter, so any is an
    // `InvalidRequest`; it is read first, before the tenant is resolved.
    ValidatedQuery(EmptyParams {}): ValidatedQuery<EmptyParams>,
    tenant_state::ExistingTenantState(state): EntityMapTenantState,
    OriginalUri(uri): OriginalUri,
    accept: AcceptFormatNoGeo,
    ResponseContextRequest { context, api_root }: ResponseContextRequest,
    prefer: PreferHeader,
    TypedPath(entity_map_id): TypedPath<Identifier>,
) -> Result<Response, ProblemResponse> {
    let map = state.entity_maps.retrieve(&entity_map_id).await.with_entity_map_instance(&uri)?;

    let value = serde_json::to_value(EntityMapBody::new(&map, prefer.requested_version))
        .map_err(|error| internal_problem(&error, InternalFailureSummary::RESPONSE_ENCODING, &uri))?;
    let rendered = ResponseContextSource::new(state.entity_query.engine(), &context, &state.context_hosting, &api_root)
        .start_rendering_normalized(accept.media_type(), prefer.rendering_preferences())
        .await
        .with_entity_map_instance(&uri)?
        .with_payload(ResponsePayload::Single(value))
        .render()
        .with_entity_map_instance(&uri)?;
    Ok(NgsiLdResponse::ok(rendered).into_response())
}

/// PATCH /ngsi-ld/v1/entityMaps/{entityMapId} -- Update an entity map (section 5.14.2).
pub async fn update(
    // Clause 6.3.20: the operation defines no parameter, so any is an
    // `InvalidRequest`; it is read first, before the tenant is resolved.
    ValidatedQuery(EmptyParams {}): ValidatedQuery<EmptyParams>,
    tenant_state::ExistingTenantState(state): EntityMapTenantState,
    OriginalUri(uri): OriginalUri,
    TypedPath(entity_map_id): TypedPath<Identifier>,
    // Clause 5.14.2.3 takes "A JSON-LD document"; the update names no term,
    // so its context is never resolved.
    JsonLd { body, context }: JsonLd<UpdateEntityMapBody>,
) -> Result<Response, ProblemResponse> {
    // The update names no term a context could define, but clause 5.5.7
    // refuses an `@context` embedded below its top level all the same.
    let body = body.context_free(context)?;
    let expires_at = body.expires_at.ok_or_else(|| bad_request_data("expiresAt is required", &uri))?;

    if expires_at <= chrono::Utc::now() {
        return Err(bad_request_data("expiresAt must be in the future", &uri));
    }

    state.entity_maps.update(&entity_map_id, expires_at).await.with_entity_map_instance(&uri)?;

    Ok(StatusCode::NO_CONTENT.into_response())
}

/// DELETE /ngsi-ld/v1/entityMaps/{entityMapId} -- Delete an entity map (section 5.14.3).
pub async fn delete(
    // Clause 6.3.20: the operation defines no parameter, so any is an
    // `InvalidRequest`; it is read first, before the tenant is resolved.
    ValidatedQuery(EmptyParams {}): ValidatedQuery<EmptyParams>,
    tenant_state::ExistingTenantState(state): EntityMapTenantState,
    OriginalUri(uri): OriginalUri,
    TypedPath(entity_map_id): TypedPath<Identifier>,
) -> Result<Response, ProblemResponse> {
    state.entity_maps.delete(&entity_map_id).await.with_entity_map_instance(&uri)?;

    Ok(StatusCode::NO_CONTENT.into_response())
}

/// Resolve the local membership `window` selects of an entity query (clause
/// 5.14.4.4).
async fn local_entity_ids(
    state: &Arc<EntityMapRouteServices>,
    query: &EntityQueryParameters,
    window: &PageWindow,
    context: &ContextSpec,
    uri: &HttpUri,
) -> Result<Vec<Identifier>, ProblemResponse> {
    let compiled = state.entity_query.compile_query_public(query, context).await.with_entity_map_instance(uri)?;
    state
        .entity_query
        .query_ids(&state.live_target, &compiled, window)
        .await
        .with_entity_map_instance(uri)
}

/// Create the map an entity query selects, over either binding (clause 5.14.4.4).
async fn create_from_entity_query(
    state: &Arc<EntityMapRouteServices>,
    query: &mut EntityQueryParameters,
    window: &PageWindow,
    forwarded_query: String,
    rendering: &CreationRendering<'_>,
    triggering: TriggeringRequest<'_>,
) -> Result<Response, ProblemResponse> {
    // The fingerprint is taken from the request as written, before any filter is
    // held back, so a later query reusing this map -- which fingerprints with its
    // `q`, geoquery and `scopeQ` present -- matches it.
    let fingerprint = fingerprint_of(FingerprintedRequest::QueryEntities(query)).with_entity_map_instance(rendering.uri)?;
    // Clause 5.5.9.3: a split map fixes candidate entities, so the attribute
    // reading filters are held back before the local membership is resolved. A
    // locally held partial entity that fails `q` may be completed by a peer, so
    // it must stay in the candidate set the map fixes rather than be filtered out
    // here -- the deferred filters are evaluated once, over the merged result, by
    // each query that later reuses the map.
    if state.forwarding.split_entities(query.split_entities).is_split() && !query.is_local() {
        let _deferred = query.defer_split_filters();
    }
    let local_ids = local_entity_ids(state, query, window, rendering.context, rendering.uri).await?;
    create_map(
        state,
        MapCreation {
            local_ids,
            lifetime: query.entity_map_lifetime.as_ref(),
            fingerprint,
            kind: EntityMapQueryKind::Entities,
            entity_type: query.entity_type.as_ref(),
            entity_ids: query.id.as_deref(),
            forwarded_query,
            local: query.is_local(),
            resource: "entityMaps",
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
        // Clause 5.14.5.4: the map being created fixes exactly what this
        // request matched, identifiers included.
        .query_ids(&state.live_target, params, window, rendering.context, RequestedIdentifiers::Narrow)
        .await
        .with_entity_map_instance(rendering.uri)?;
    create_map(
        state,
        MapCreation {
            local_ids,
            lifetime: params.entity_map_lifetime.as_ref(),
            fingerprint,
            kind: EntityMapQueryKind::TemporalEntities,
            entity_type: params.entity_type.as_ref(),
            entity_ids: params.id.as_deref(),
            forwarded_query,
            local: params.is_local(),
            resource: "temporal/entityMaps",
        },
        rendering,
        triggering,
    )
    .await
}

/// GET /ngsi-ld/v1/entityMaps -- Create an entity map from query entities (section 5.14.4, section 6.34.3.1).
pub async fn create_from_query(
    tenant_state::ExistingTenantState(state): EntityMapTenantState,
    accept: AcceptFormatNoGeo,
    NgsiLdRequestEnvelope { uri, incoming_via, headers }: NgsiLdRequestEnvelope,
    ResponseContextRequest { context, api_root }: ResponseContextRequest,
    prefer: PreferHeader,
    RequestPagination(pagination): RequestPagination,
    EntityQuery(mut query): EntityQuery,
) -> Result<Response, ProblemResponse> {
    let context = admit_user_context(state.entity_query.engine(), SubmittedContext::from(context))
        .await
        .with_entity_map_instance(&uri)?;
    validate_entity_map_query(&query, &uri)?;
    let window = membership_window(pagination, &query.page, &query.ordering, &uri)?;
    let filters = ForwardedFilters::for_split(state.forwarding.split_entities(query.split_entities).is_split());
    let forwarded_query = forwarded_query_string(&uri, filters);
    create_from_entity_query(
        &state,
        &mut query,
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

/// POST /ngsi-ld/v1/entityMaps -- Create an entity map from query body (section 5.14.4, section 6.34.3.2).
pub async fn create_from_query_post(
    tenant_state::ExistingTenantState(state): EntityMapTenantState,
    accept: AcceptFormatNoGeo,
    NgsiLdRequestEnvelope { uri, incoming_via, headers }: NgsiLdRequestEnvelope,
    api_root: ApiRoot,
    prefer: PreferHeader,
    RequestPagination(pagination): RequestPagination,
    JsonLd { body, context }: JsonLd<Query>,
) -> Result<Response, ProblemResponse> {
    // Clause 6.3.5: the query is read against the one context the body
    // extractor resolved, admitted first (clause 5.5.7).
    let context = admit_user_context(state.entity_query.engine(), context).await.with_entity_map_instance(&uri)?;
    let body = body.admitted(&context)?;
    let mut query = EntityQueryParameters::try_from(body).with_entity_map_instance(&uri)?;
    validate_entity_map_query(&query, &uri)?;
    let window = membership_window(pagination, &query.page, &query.ordering, &uri)?;
    let filters = ForwardedFilters::for_split(state.forwarding.split_entities(query.split_entities).is_split());
    let forwarded_query = body_query_string(&query, filters, &uri);
    create_from_entity_query(
        &state,
        &mut query,
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

/// GET /ngsi-ld/v1/temporal/entityMaps -- Create an entity map from temporal query (section 5.14.5, section 6.35.3.1).
pub async fn create_temporal_from_query(
    tenant_state::ExistingTenantState(state): EntityMapTenantState,
    accept: AcceptFormatNoGeo,
    NgsiLdRequestEnvelope { uri, incoming_via, headers }: NgsiLdRequestEnvelope,
    ResponseContextRequest { context, api_root }: ResponseContextRequest,
    prefer: PreferHeader,
    RequestPagination(pagination): RequestPagination,
    TemporalQuery(params): TemporalQuery,
) -> Result<Response, ProblemResponse> {
    let context = admit_user_context(state.entity_query.engine(), SubmittedContext::from(context))
        .await
        .with_entity_map_instance(&uri)?;
    validate_temporal_entity_map_query(&params, state.temporal.linked_retrieval(), &uri)?;
    let window = membership_window(pagination, &params.page, &params.ordering, &uri)?;
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

/// POST /ngsi-ld/v1/temporal/entityMaps -- Create an entity map from temporal query body (section 5.14.5, section 6.35.3.2).
pub async fn create_temporal_from_query_post(
    tenant_state::ExistingTenantState(state): EntityMapTenantState,
    accept: AcceptFormatNoGeo,
    NgsiLdRequestEnvelope { uri, incoming_via, headers }: NgsiLdRequestEnvelope,
    api_root: ApiRoot,
    prefer: PreferHeader,
    RequestPagination(pagination): RequestPagination,
    JsonLd { body, context }: JsonLd<Query>,
) -> Result<Response, ProblemResponse> {
    // Clause 6.3.5: the query is read against the one context the body
    // extractor resolved, admitted first (clause 5.5.7).
    let context = admit_user_context(state.entity_query.engine(), context).await.with_entity_map_instance(&uri)?;
    let body = body.admitted(&context)?;
    let params = TemporalQueryParameters::try_from(body).with_entity_map_instance(&uri)?;
    validate_temporal_entity_map_query(&params, state.temporal.linked_retrieval(), &uri)?;
    let window = membership_window(pagination, &params.page, &params.ordering, &uri)?;
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
