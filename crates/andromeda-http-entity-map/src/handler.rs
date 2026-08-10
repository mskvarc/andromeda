//! Handlers for the `/entityMaps` endpoints.

use crate::{
    error::{EntityMapResultExt, bad_request_data, internal_problem, invalid_request},
    state::EntityMapTenantState,
    validation::validate_entity_map_has_filter,
};
use andromeda_http_extraction::{
    accept::AcceptFormat,
    context::ContextUrl,
    entity_query::EntityQuery,
    json_body::classify_json_body,
    prefer::PreferHeader,
    typed_path::TypedPath,
};
use andromeda_http_response::{problem_response::ProblemResponse, render::start_rendering, single::NgsiLdResponse};
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_model::query::body::Query;
use andromeda_ngsi_ld_render::response::ResponsePayload;
use andromeda_ngsi_ld_request::{body::entity_map::UpdateEntityMapBody, query::entity::parameters::EntityQueryParameters};
use axum::{
    Json,
    extract::{OriginalUri, rejection::JsonRejection},
    http::StatusCode,
    response::{IntoResponse, Response},
};

/// GET /ngsi-ld/v1/entityMaps/{entityMapId} -- Retrieve an entity map (section 5.14.1).
pub async fn get(
    andromeda_http_extraction::tenant_state::ExistingTenantState(state): EntityMapTenantState,
    OriginalUri(uri): OriginalUri,
    accept: AcceptFormat,
    context: ContextUrl,
    TypedPath(entity_map_id): TypedPath<Uri>,
) -> Result<Response, ProblemResponse> {
    let context: ContextSpec = context.into_inner();

    let map = state.entity_maps.retrieve(&entity_map_id).await.with_entity_map_instance(&uri)?;

    let value = serde_json::to_value(&map).map_err(|error| internal_problem(&error, &uri))?;
    let rendered = start_rendering(accept.media_type(), &context, PreferHeader::default().rendering_preferences())
        .with_payload(ResponsePayload::Single(value))
        .render()
        .with_entity_map_instance(&uri)?;
    Ok(NgsiLdResponse::ok(rendered).into_response())
}

/// PATCH /ngsi-ld/v1/entityMaps/{entityMapId} -- Update an entity map (section 5.14.2).
pub async fn update(
    andromeda_http_extraction::tenant_state::ExistingTenantState(state): EntityMapTenantState,
    OriginalUri(uri): OriginalUri,
    TypedPath(entity_map_id): TypedPath<Uri>,
    body: Result<Json<UpdateEntityMapBody>, JsonRejection>,
) -> Result<Response, ProblemResponse> {
    let Json(body) =
        body.map_err(|rejection| classify_json_body(rejection).map(|detail| invalid_request(detail, &uri), |detail| bad_request_data(detail, &uri)))?;
    let expires_at = body.expires_at.ok_or_else(|| bad_request_data("expiresAt is required", &uri))?;

    if expires_at <= chrono::Utc::now() {
        return Err(bad_request_data("expiresAt must be in the future", &uri));
    }

    state.entity_maps.update(&entity_map_id, expires_at).await.with_entity_map_instance(&uri)?;

    Ok(StatusCode::NO_CONTENT.into_response())
}

/// DELETE /ngsi-ld/v1/entityMaps/{entityMapId} -- Delete an entity map (section 5.14.3).
pub async fn delete(
    andromeda_http_extraction::tenant_state::ExistingTenantState(state): EntityMapTenantState,
    OriginalUri(uri): OriginalUri,
    TypedPath(entity_map_id): TypedPath<Uri>,
) -> Result<Response, ProblemResponse> {
    state.entity_maps.delete(&entity_map_id).await.with_entity_map_instance(&uri)?;

    Ok(StatusCode::NO_CONTENT.into_response())
}

/// GET /ngsi-ld/v1/entityMaps -- Create an entity map from query entities (section 5.14.4, section 6.34.3.1).
pub async fn create_from_query(
    andromeda_http_extraction::tenant_state::ExistingTenantState(state): EntityMapTenantState,
    OriginalUri(uri): OriginalUri,
    accept: AcceptFormat,
    context: ContextUrl,
    EntityQuery(query): EntityQuery,
) -> Result<Response, ProblemResponse> {
    // section 5.14.4.4: At least one filter must be provided for EntityMap creation
    validate_entity_map_has_filter(&query, &uri)?;

    let context: ContextSpec = context.into_inner();

    let compiled = state
        .entity_query
        .compile_query_uncapped(&query, &context)
        .await
        .with_entity_map_instance(&uri)?;
    let ids = state.entity_query.query_ids(&compiled).await.with_entity_map_instance(&uri)?;

    let lifetime = query.entity_map_lifetime.as_ref();
    let map = state.entity_maps.create(ids, lifetime).await.with_entity_map_instance(&uri)?;

    let location = format!("/ngsi-ld/v1/entityMaps/{}", map.id);
    // Clone id before converting map to Value
    let map_uri = map.id.clone();

    let value = serde_json::to_value(&map).map_err(|error| internal_problem(&error, &uri))?;
    let rendered = start_rendering(accept.media_type(), &context, PreferHeader::default().rendering_preferences())
        .with_payload(ResponsePayload::Single(value))
        .render()
        .with_entity_map_instance(&uri)?;

    Ok(NgsiLdResponse::new(StatusCode::CREATED, rendered)
        .with_location(location)
        .with_entity_map_uri(map_uri)
        .into_response())
}

/// POST /ngsi-ld/v1/entityMaps -- Create an entity map from query body (section 5.14.4, section 6.34.3.2).
pub async fn create_from_query_post(
    andromeda_http_extraction::tenant_state::ExistingTenantState(state): EntityMapTenantState,
    OriginalUri(uri): OriginalUri,
    accept: AcceptFormat,
    context: ContextUrl,
    body: Result<Json<Query>, JsonRejection>,
) -> Result<Response, ProblemResponse> {
    let Json(body) =
        body.map_err(|rejection| classify_json_body(rejection).map(|detail| invalid_request(detail, &uri), |detail| bad_request_data(detail, &uri)))?;
    let context_spec: ContextSpec = context.into_inner();

    let lifetime = body.entity_map_lifetime;
    let query = EntityQueryParameters::try_from(body).with_entity_map_instance(&uri)?;
    query.validate().with_entity_map_instance(&uri)?;
    validate_entity_map_has_filter(&query, &uri)?;

    let compiled = state
        .entity_query
        .compile_query_uncapped(&query, &context_spec)
        .await
        .with_entity_map_instance(&uri)?;
    let ids = state.entity_query.query_ids(&compiled).await.with_entity_map_instance(&uri)?;

    let map = state.entity_maps.create(ids, lifetime.as_ref()).await.with_entity_map_instance(&uri)?;

    let location = format!("/ngsi-ld/v1/entityMaps/{}", map.id);
    let map_uri = map.id.clone();

    let value = serde_json::to_value(&map).map_err(|error| internal_problem(&error, &uri))?;
    let rendered = start_rendering(accept.media_type(), &context_spec, PreferHeader::default().rendering_preferences())
        .with_payload(ResponsePayload::Single(value))
        .render()
        .with_entity_map_instance(&uri)?;

    Ok(NgsiLdResponse::new(StatusCode::CREATED, rendered)
        .with_location(location)
        .with_entity_map_uri(map_uri)
        .into_response())
}

/// GET /ngsi-ld/v1/temporal/entityMaps -- Create an entity map from temporal query (section 5.14.5, section 6.35.3.1).
pub async fn create_temporal_from_query(
    andromeda_http_extraction::tenant_state::ExistingTenantState(state): EntityMapTenantState,
    OriginalUri(uri): OriginalUri,
    accept: AcceptFormat,
    context: ContextUrl,
    EntityQuery(query): EntityQuery,
) -> Result<Response, ProblemResponse> {
    validate_entity_map_has_filter(&query, &uri)?;

    let context: ContextSpec = context.into_inner();

    let compiled = state
        .entity_query
        .compile_query_uncapped(&query, &context)
        .await
        .with_entity_map_instance(&uri)?;
    let ids = state.entity_query.query_ids(&compiled).await.with_entity_map_instance(&uri)?;

    let lifetime = query.entity_map_lifetime.as_ref();
    let map = state.entity_maps.create(ids, lifetime).await.with_entity_map_instance(&uri)?;

    let location = format!("/ngsi-ld/v1/temporal/entityMaps/{}", map.id);
    let map_uri = map.id.clone();

    let value = serde_json::to_value(&map).map_err(|error| internal_problem(&error, &uri))?;
    let rendered = start_rendering(accept.media_type(), &context, PreferHeader::default().rendering_preferences())
        .with_payload(ResponsePayload::Single(value))
        .render()
        .with_entity_map_instance(&uri)?;

    Ok(NgsiLdResponse::new(StatusCode::CREATED, rendered)
        .with_location(location)
        .with_entity_map_uri(map_uri)
        .into_response())
}

/// POST /ngsi-ld/v1/temporal/entityMaps -- Create an entity map from temporal query body (section 5.14.5, section 6.35.3.2).
pub async fn create_temporal_from_query_post(
    andromeda_http_extraction::tenant_state::ExistingTenantState(state): EntityMapTenantState,
    OriginalUri(uri): OriginalUri,
    accept: AcceptFormat,
    context: ContextUrl,
    body: Result<Json<Query>, JsonRejection>,
) -> Result<Response, ProblemResponse> {
    let Json(body) =
        body.map_err(|rejection| classify_json_body(rejection).map(|detail| invalid_request(detail, &uri), |detail| bad_request_data(detail, &uri)))?;
    let context_spec: ContextSpec = context.into_inner();

    let lifetime = body.entity_map_lifetime;
    let query = EntityQueryParameters::try_from(body).with_entity_map_instance(&uri)?;
    query.validate().with_entity_map_instance(&uri)?;
    validate_entity_map_has_filter(&query, &uri)?;

    let compiled = state
        .entity_query
        .compile_query_uncapped(&query, &context_spec)
        .await
        .with_entity_map_instance(&uri)?;
    let ids = state.entity_query.query_ids(&compiled).await.with_entity_map_instance(&uri)?;

    let map = state.entity_maps.create(ids, lifetime.as_ref()).await.with_entity_map_instance(&uri)?;

    let location = format!("/ngsi-ld/v1/temporal/entityMaps/{}", map.id);
    let map_uri = map.id.clone();

    let value = serde_json::to_value(&map).map_err(|error| internal_problem(&error, &uri))?;
    let rendered = start_rendering(accept.media_type(), &context_spec, PreferHeader::default().rendering_preferences())
        .with_payload(ResponsePayload::Single(value))
        .render()
        .with_entity_map_instance(&uri)?;

    Ok(NgsiLdResponse::new(StatusCode::CREATED, rendered)
        .with_location(location)
        .with_entity_map_uri(map_uri)
        .into_response())
}
