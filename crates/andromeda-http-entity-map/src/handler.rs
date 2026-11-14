//! Handlers for one `EntityMap` resource: retrieve, update and delete it
//! (clauses 5.14.1 to 5.14.3).

use crate::{error::EntityMapResultExt, expiry_update_refusal::ExpiryUpdateRefusal, state::EntityMapTenantState};
use andromeda_http_context_registry::response_rendering::ResponseContextSource;
use andromeda_http_extraction::{
    accept::AcceptFormatNoGeo,
    jsonld::JsonLd,
    path_segment::EntityMapId,
    prefer::PreferHeader,
    query::ValidatedQuery,
    response_context_request::ResponseContextRequest,
    tenant_state,
    typed_path::TypedPath,
};
use andromeda_http_response::{
    problem_response::ProblemResponse,
    rendered_response::RenderedResponse,
    request_refusal::RequestRefusal,
    single::NgsiLdResponse,
};
use andromeda_ngsi_ld_render::entity_map_body::EntityMapBody;
use andromeda_ngsi_ld_request::{body::entity_map::UpdateEntityMapBody, empty::EmptyParams};
use axum::{
    extract::OriginalUri,
    http::StatusCode,
    response::{IntoResponse, Response},
};

/// GET /ngsi-ld/v1/entityMaps/{entityMapId}: retrieve an entity map (section 5.14.1).
pub(crate) async fn get(
    // Clause 6.3.20: the operation defines no parameter, so any is an
    // `InvalidRequest`; it is read first, before the tenant is resolved.
    ValidatedQuery(EmptyParams {}): ValidatedQuery<EmptyParams>,
    tenant_state::ExistingTenantState(state): EntityMapTenantState,
    OriginalUri(uri): OriginalUri,
    accept: AcceptFormatNoGeo,
    ResponseContextRequest { context, api_root }: ResponseContextRequest,
    prefer: PreferHeader,
    TypedPath(EntityMapId(entity_map_id)): TypedPath<EntityMapId>,
) -> Result<Response, ProblemResponse> {
    let map = state.entity_maps.retrieve(&entity_map_id).await.with_entity_map_instance(&uri)?;

    let body = EntityMapBody::new(&map, prefer.requested_version);
    let renderer = ResponseContextSource::new(state.entity_query.engine(), &context, &state.context_hosting, &api_root)
        .start_rendering_normalized(accept.media_type(), prefer.rendering_preferences())
        .await
        .with_entity_map_instance(&uri)?;
    let written = RenderedResponse::resource(renderer.with_body(&body)).with_entity_map_instance(&uri)?;
    Ok(NgsiLdResponse::ok(written).into_response())
}

/// PATCH /ngsi-ld/v1/entityMaps/{entityMapId}: update an entity map (section 5.14.2).
pub(crate) async fn update(
    // Clause 6.3.20: the operation defines no parameter, so any is an
    // `InvalidRequest`; it is read first, before the tenant is resolved.
    ValidatedQuery(EmptyParams {}): ValidatedQuery<EmptyParams>,
    tenant_state::ExistingTenantState(state): EntityMapTenantState,
    OriginalUri(uri): OriginalUri,
    TypedPath(EntityMapId(entity_map_id)): TypedPath<EntityMapId>,
    // Clause 5.14.2.3 takes "A JSON-LD document"; the update names no term,
    // so its context is never resolved.
    JsonLd { body, context }: JsonLd<UpdateEntityMapBody>,
) -> Result<Response, ProblemResponse> {
    // The update names no term a context could define, but clause 5.5.7
    // refuses an `@context` embedded below its top level all the same.
    let body = body.context_free(context).await?;
    let expires_at = match body.expires_at {
        Some(expires_at) if expires_at > chrono::Utc::now() => expires_at,
        Some(_) => return Err(ExpiryUpdateRefusal::NotInTheFuture.into_problem(uri)),
        None => return Err(ExpiryUpdateRefusal::Missing.into_problem(uri)),
    };

    state.entity_maps.update(&entity_map_id, expires_at).await.with_entity_map_instance(&uri)?;

    Ok(StatusCode::NO_CONTENT.into_response())
}

/// DELETE /ngsi-ld/v1/entityMaps/{entityMapId}: delete an entity map (section 5.14.3).
pub(crate) async fn delete(
    // Clause 6.3.20: the operation defines no parameter, so any is an
    // `InvalidRequest`; it is read first, before the tenant is resolved.
    ValidatedQuery(EmptyParams {}): ValidatedQuery<EmptyParams>,
    tenant_state::ExistingTenantState(state): EntityMapTenantState,
    OriginalUri(uri): OriginalUri,
    TypedPath(EntityMapId(entity_map_id)): TypedPath<EntityMapId>,
) -> Result<Response, ProblemResponse> {
    state.entity_maps.delete(&entity_map_id).await.with_entity_map_instance(&uri)?;

    Ok(StatusCode::NO_CONTENT.into_response())
}
