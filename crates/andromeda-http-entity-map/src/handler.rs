//! Handlers for the `/entityMaps` and `/temporal/entityMaps` endpoints (clauses 5.14, 6.34, 6.35).

use crate::{
    creation::{CreationRendering, MapCreation, create_map},
    error::{EntityMapResultExt, bad_request_data, internal_problem, invalid_request},
    forwarded_query_string::{ForwardedFilters, body_query_string, forwarded_query_string, forwarded_temporal_query_string, temporal_body_query_string},
    state::{EntityMapRouteServices, EntityMapTenantState},
    validation::{validate_entity_map_has_filter, validate_entity_map_query, validate_temporal_entity_map_query},
};
use andromeda_entity_map::fingerprint::{FingerprintedRequest, fingerprint_of};
use andromeda_federation::{entity_map::EntityMapQueryKind, request::TriggeringRequest};
use andromeda_http_extraction::{
    accept::AcceptFormatNoGeo,
    context::ContextUrl,
    entity_query::EntityQuery,
    json_body::classify_json_body,
    jsonld::JsonLd,
    prefer::PreferHeader,
    request_envelope::NgsiLdRequestEnvelope,
    temporal_query::TemporalQuery,
    tenant_state,
    typed_path::TypedPath,
};
use andromeda_http_response::{problem_response::ProblemResponse, render::start_rendering, single::NgsiLdResponse};
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_model::query::body::Query;
use andromeda_ngsi_ld_render::response::ResponsePayload;
use andromeda_ngsi_ld_request::{
    body::entity_map::UpdateEntityMapBody,
    query::{entity::parameters::EntityQueryParameters, temporal::TemporalQueryParameters},
};
use axum::{
    Json,
    extract::{OriginalUri, rejection::JsonRejection},
    http::{StatusCode, Uri as HttpUri},
    response::{IntoResponse, Response},
};
use std::sync::Arc;

/// GET /ngsi-ld/v1/entityMaps/{entityMapId} -- Retrieve an entity map (section 5.14.1).
pub async fn get(
    tenant_state::ExistingTenantState(state): EntityMapTenantState,
    OriginalUri(uri): OriginalUri,
    accept: AcceptFormatNoGeo,
    context: ContextUrl,
    prefer: PreferHeader,
    TypedPath(entity_map_id): TypedPath<Uri>,
) -> Result<Response, ProblemResponse> {
    let context: ContextSpec = context.into_inner();

    let map = state.entity_maps.retrieve(&entity_map_id).await.with_entity_map_instance(&uri)?;

    let value = serde_json::to_value(&map).map_err(|error| internal_problem(&error, &uri))?;
    let rendered = start_rendering(accept.media_type(), &context, prefer.rendering_preferences())
        .with_payload(ResponsePayload::Single(value))
        .render()
        .with_entity_map_instance(&uri)?;
    Ok(NgsiLdResponse::ok(rendered).into_response())
}

/// PATCH /ngsi-ld/v1/entityMaps/{entityMapId} -- Update an entity map (section 5.14.2).
pub async fn update(
    tenant_state::ExistingTenantState(state): EntityMapTenantState,
    OriginalUri(uri): OriginalUri,
    TypedPath(entity_map_id): TypedPath<Uri>,
    body: Result<Json<UpdateEntityMapBody>, JsonRejection>,
) -> Result<Response, ProblemResponse> {
    let Json(body) =
        body.map_err(|rejection| classify_json_body(&rejection).map(|detail| invalid_request(detail, &uri), |detail| bad_request_data(detail, &uri)))?;
    let expires_at = body.expires_at.ok_or_else(|| bad_request_data("expiresAt is required", &uri))?;

    if expires_at <= chrono::Utc::now() {
        return Err(bad_request_data("expiresAt must be in the future", &uri));
    }

    state.entity_maps.update(&entity_map_id, expires_at).await.with_entity_map_instance(&uri)?;

    Ok(StatusCode::NO_CONTENT.into_response())
}

/// DELETE /ngsi-ld/v1/entityMaps/{entityMapId} -- Delete an entity map (section 5.14.3).
pub async fn delete(
    tenant_state::ExistingTenantState(state): EntityMapTenantState,
    OriginalUri(uri): OriginalUri,
    TypedPath(entity_map_id): TypedPath<Uri>,
) -> Result<Response, ProblemResponse> {
    state.entity_maps.delete(&entity_map_id).await.with_entity_map_instance(&uri)?;

    Ok(StatusCode::NO_CONTENT.into_response())
}

/// Resolve the local membership of an entity query (clause 5.14.4.4).
async fn local_entity_ids(
    state: &Arc<EntityMapRouteServices>,
    query: &EntityQueryParameters,
    context: &ContextSpec,
    uri: &HttpUri,
) -> Result<Vec<Uri>, ProblemResponse> {
    let compiled = state.entity_query.compile_query_uncapped(query, context).await.with_entity_map_instance(uri)?;
    state.entity_query.query_ids(&state.live_target, &compiled).await.with_entity_map_instance(uri)
}

/// Create the map an entity query selects, over either binding (clause 5.14.4.4).
async fn create_from_entity_query(
    state: &Arc<EntityMapRouteServices>,
    query: &EntityQueryParameters,
    forwarded_query: String,
    rendering: &CreationRendering<'_>,
    triggering: TriggeringRequest<'_>,
) -> Result<Response, ProblemResponse> {
    // Clause 5.14.4.4: at least one filter must be provided for EntityMap creation.
    validate_entity_map_has_filter(query, rendering.uri)?;
    let fingerprint = fingerprint_of(FingerprintedRequest::QueryEntities(query)).with_entity_map_instance(rendering.uri)?;
    let local_ids = local_entity_ids(state, query, rendering.context, rendering.uri).await?;
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
    forwarded_query: String,
    rendering: &CreationRendering<'_>,
    triggering: TriggeringRequest<'_>,
) -> Result<Response, ProblemResponse> {
    validate_temporal_entity_map_query(params, rendering.uri)?;
    let fingerprint = fingerprint_of(FingerprintedRequest::QueryTemporal(params)).with_entity_map_instance(rendering.uri)?;
    // Table 6.18.3.2-1: the temporal window, `timeproperty` and `lastN` bound
    // the candidates the same way they bound a temporal query.
    let local_ids = state
        .temporal
        .query_ids(&state.live_target, params, rendering.context)
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
    NgsiLdRequestEnvelope {
        uri,
        context,
        incoming_via,
        headers,
    }: NgsiLdRequestEnvelope,
    accept: AcceptFormatNoGeo,
    prefer: PreferHeader,
    EntityQuery(query): EntityQuery,
) -> Result<Response, ProblemResponse> {
    validate_entity_map_query(&query, &uri)?;
    let context: ContextSpec = context.into_inner();
    let filters = ForwardedFilters::for_split(state.forwarding.split_entities(query.split_entities).is_split());
    let forwarded_query = forwarded_query_string(&uri, filters);
    create_from_entity_query(
        &state,
        &query,
        forwarded_query,
        &CreationRendering {
            context: &context,
            accept,
            prefer,
            uri: &uri,
        },
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
    NgsiLdRequestEnvelope {
        uri,
        context,
        incoming_via,
        headers,
    }: NgsiLdRequestEnvelope,
    accept: AcceptFormatNoGeo,
    prefer: PreferHeader,
    JsonLd {
        value: body,
        context: body_context,
    }: JsonLd<Query>,
) -> Result<Response, ProblemResponse> {
    // Clause 6.3.5: the body's `@context` and the `Link` context make one.
    let context = context.merge(body_context);
    let query = EntityQueryParameters::try_from(body).with_entity_map_instance(&uri)?;
    validate_entity_map_query(&query, &uri)?;
    let filters = ForwardedFilters::for_split(state.forwarding.split_entities(query.split_entities).is_split());
    let forwarded_query = body_query_string(&query, filters, &uri);
    create_from_entity_query(
        &state,
        &query,
        forwarded_query,
        &CreationRendering {
            context: &context,
            accept,
            prefer,
            uri: &uri,
        },
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
    NgsiLdRequestEnvelope {
        uri,
        context,
        incoming_via,
        headers,
    }: NgsiLdRequestEnvelope,
    accept: AcceptFormatNoGeo,
    prefer: PreferHeader,
    TemporalQuery(params): TemporalQuery,
) -> Result<Response, ProblemResponse> {
    let context: ContextSpec = context.into_inner();
    let filters = ForwardedFilters::for_split(state.forwarding.split_entities(params.split_entities).is_split());
    let forwarded_query = forwarded_temporal_query_string(&uri, filters);
    create_from_temporal_query(
        &state,
        &params,
        forwarded_query,
        &CreationRendering {
            context: &context,
            accept,
            prefer,
            uri: &uri,
        },
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
    NgsiLdRequestEnvelope {
        uri,
        context,
        incoming_via,
        headers,
    }: NgsiLdRequestEnvelope,
    accept: AcceptFormatNoGeo,
    prefer: PreferHeader,
    JsonLd {
        value: body,
        context: body_context,
    }: JsonLd<Query>,
) -> Result<Response, ProblemResponse> {
    // Clause 6.3.5: the body's `@context` and the `Link` context make one.
    let context = context.merge(body_context);
    let params = TemporalQueryParameters::try_from(body).with_entity_map_instance(&uri)?;
    // Clause 5.14.5.4: a temporal query and one selection criterion are mandatory.
    params.validate(true).with_entity_map_instance(&uri)?;
    let filters = ForwardedFilters::for_split(state.forwarding.split_entities(params.split_entities).is_split());
    let forwarded_query = temporal_body_query_string(&params, filters);
    create_from_temporal_query(
        &state,
        &params,
        forwarded_query,
        &CreationRendering {
            context: &context,
            accept,
            prefer,
            uri: &uri,
        },
        TriggeringRequest {
            via: incoming_via.as_ref(),
            headers: Some(&headers),
        },
    )
    .await
}
