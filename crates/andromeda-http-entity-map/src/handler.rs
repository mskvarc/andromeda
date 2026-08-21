//! Handlers for the `/entityMaps` and `/temporal/entityMaps` endpoints (clauses 5.14, 6.34, 6.35).

use crate::{
    error::{EntityMapResultExt, bad_request_data, internal_problem, invalid_request},
    state::{EntityMapRouteServices, EntityMapTenantState},
    validation::validate_entity_map_has_filter,
};
use andromeda_federation::{
    entity_map::{EntityMapQueryKind, ForwardEntityMapRequest, ForwardedEntityMap},
    request::TriggeringRequest,
};
use andromeda_http_extraction::{
    accept::AcceptFormatNoGeo,
    context::ContextUrl,
    entity_query::EntityQuery,
    json_body::classify_json_body,
    prefer::PreferHeader,
    temporal_query::TemporalQuery,
    tenant_state,
    typed_path::TypedPath,
    via::IncomingVia,
};
use andromeda_http_headers::ngsild_warning::NgsiLdWarning;
use andromeda_http_response::{problem_response::ProblemResponse, render::start_rendering, single::NgsiLdResponse};
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_model::{entity_map::EntityMap, query::body::Query};
use andromeda_ngsi_ld_query::type_selection::grammar::TypeExprRaw;
use andromeda_ngsi_ld_render::response::ResponsePayload;
use andromeda_ngsi_ld_request::{
    body::entity_map::UpdateEntityMapBody,
    query::{entity::parameters::EntityQueryParameters, temporal::TemporalQueryParameters},
};
use axum::{
    Json,
    extract::{OriginalUri, rejection::JsonRejection},
    http::{HeaderMap, StatusCode, Uri as HttpUri},
    response::{IntoResponse, Response},
};
use iso8601_duration::Duration as Iso8601Duration;
use std::sync::Arc;
use url::form_urlencoded;

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
    tenant_state::ExistingTenantState(state): EntityMapTenantState,
    OriginalUri(uri): OriginalUri,
    TypedPath(entity_map_id): TypedPath<Uri>,
) -> Result<Response, ProblemResponse> {
    state.entity_maps.delete(&entity_map_id).await.with_entity_map_instance(&uri)?;

    Ok(StatusCode::NO_CONTENT.into_response())
}

/// What one creation request resolved to, whichever binding it came through.
struct MapCreation<'a> {
    /// The entities this broker holds that match.
    local_ids: Vec<Uri>,
    /// The requested lifetime, if any.
    lifetime: Option<&'a Iso8601Duration>,
    /// Which creation resource the peers are asked through.
    kind: EntityMapQueryKind,
    /// Compact entity type for registration matching.
    entity_type: Option<&'a TypeExprRaw>,
    /// Entity identifiers for registration matching.
    entity_ids: Option<&'a [Uri]>,
    /// The query string forwarded to each source.
    forwarded_query: String,
    /// Whether the request asked for local scope (clause 5.5.13).
    local: bool,
    /// The resource the map is created under, for `Location`.
    resource: &'static str,
}

/// The query string forwarded to each contributing source (clause 5.14.4.4):
/// the whole query when entities are not split, the entity selectors alone when
/// they are, because a filter cannot be decided on a partial entity.
fn forwarded_query_string(uri: &HttpUri, split_entities: bool) -> String {
    let original = uri.query().unwrap_or("");
    if !split_entities {
        return original.to_string();
    }
    let mut serializer = form_urlencoded::Serializer::new(String::new());
    for (key, value) in form_urlencoded::parse(original.as_bytes()) {
        if matches!(key.as_ref(), "id" | "type" | "idPattern" | "entityMapLifetime") {
            serializer.append_pair(&key, &value);
        }
    }
    serializer.finish()
}

/// Compose the local map with every map the contributing sources created
/// (clause 5.14.4.4), store it and render it (clauses 6.34.3.1 and 6.35.3.1).
async fn create_map(
    state: &Arc<EntityMapRouteServices>,
    creation: MapCreation<'_>,
    context: &ContextSpec,
    accept: AcceptFormatNoGeo,
    prefer: PreferHeader,
    triggering: TriggeringRequest<'_>,
    uri: &HttpUri,
) -> Result<Response, ProblemResponse> {
    let mut map = state
        .entity_maps
        .local_map(creation.local_ids, creation.lifetime)
        .with_entity_map_instance(uri)?;
    let mut warnings: Vec<NgsiLdWarning> = Vec::new();
    // Clause 5.5.13: local scope forwards nothing.
    if !creation.local {
        let contributions: Vec<ForwardedEntityMap> = state
            .forwarding
            .forward_entity_map_query(ForwardEntityMapRequest {
                kind: creation.kind,
                entity_type_compact: creation.entity_type,
                entity_ids: creation.entity_ids,
                query_string: &creation.forwarded_query,
                context,
                triggering,
            })
            .await
            .with_entity_map_instance(uri)?;
        for contribution in contributions {
            warnings.extend(contribution.warnings);
            if let Some(remote) = contribution.map {
                map.add_remote(&contribution.registration_id, &remote);
            }
        }
    }
    state.entity_maps.store(&map).await.with_entity_map_instance(uri)?;
    render_created(&map, creation.resource, context, accept, prefer, warnings, uri)
}

/// Render a created map: 201 with `Location` and `NGSILD-EntityMap`, or 203
/// with `Preference-Applied` when the body was adapted to an older version
/// (Table 6.34.3.1-1).
fn render_created(
    map: &EntityMap,
    resource: &str,
    context: &ContextSpec,
    accept: AcceptFormatNoGeo,
    prefer: PreferHeader,
    warnings: Vec<NgsiLdWarning>,
    uri: &HttpUri,
) -> Result<Response, ProblemResponse> {
    let location = format!("/ngsi-ld/v1/{resource}/{}", map.id);
    let value = serde_json::to_value(map).map_err(|error| internal_problem(&error, uri))?;
    let rendered = start_rendering(accept.media_type(), context, prefer.rendering_preferences())
        .with_payload(ResponsePayload::Single(value))
        .render()
        .with_entity_map_instance(uri)?;
    Ok(NgsiLdResponse::new(StatusCode::CREATED, rendered)
        .with_location(location)
        .with_entity_map_uri(map.id.clone()) // clone: the header owns the identifier beside the body
        .with_warnings(warnings)
        .into_response())
}

/// Resolve the local membership of an entity query (clause 5.14.4.4).
async fn local_entity_ids(
    state: &Arc<EntityMapRouteServices>,
    query: &EntityQueryParameters,
    context: &ContextSpec,
    uri: &HttpUri,
) -> Result<Vec<Uri>, ProblemResponse> {
    let compiled = state.entity_query.compile_query_uncapped(query, context).await.with_entity_map_instance(uri)?;
    state.entity_query.query_ids(&compiled).await.with_entity_map_instance(uri)
}

/// GET /ngsi-ld/v1/entityMaps -- Create an entity map from query entities (section 5.14.4, section 6.34.3.1).
pub async fn create_from_query(
    tenant_state::ExistingTenantState(state): EntityMapTenantState,
    OriginalUri(uri): OriginalUri,
    accept: AcceptFormatNoGeo,
    context: ContextUrl,
    prefer: PreferHeader,
    IncomingVia(incoming_via): IncomingVia,
    headers: HeaderMap,
    EntityQuery(query): EntityQuery,
) -> Result<Response, ProblemResponse> {
    // section 5.14.4.4: At least one filter must be provided for EntityMap creation
    validate_entity_map_has_filter(&query, &uri)?;
    let context: ContextSpec = context.into_inner();
    let local_ids = local_entity_ids(&state, &query, &context, &uri).await?;
    let split_entities = state.forwarding.split_entities(query.split_entities);
    create_map(
        &state,
        MapCreation {
            local_ids,
            lifetime: query.entity_map_lifetime.as_ref(),
            kind: EntityMapQueryKind::Entities,
            entity_type: query.entity_type.as_ref(),
            entity_ids: query.id.as_deref(),
            forwarded_query: forwarded_query_string(&uri, split_entities),
            local: query.is_local(),
            resource: "entityMaps",
        },
        &context,
        accept,
        prefer,
        TriggeringRequest {
            via: incoming_via.as_ref(),
            headers: Some(&headers),
        },
        &uri,
    )
    .await
}

/// The query string a POST body is forwarded as: the body's entity selectors
/// rendered as GET parameters, so a peer answers the same request over either
/// binding.
fn body_query_string(query: &EntityQueryParameters, split_entities: bool, uri: &HttpUri) -> String {
    let mut serializer = form_urlencoded::Serializer::new(String::new());
    if let Some(ids) = &query.id {
        serializer.append_pair("id", &ids.iter().map(Uri::as_str).collect::<Vec<_>>().join(","));
    }
    if let Some(entity_type) = &query.entity_type {
        serializer.append_pair("type", &entity_type.to_string());
    }
    if let Some(pattern) = &query.id_pattern {
        serializer.append_pair("idPattern", pattern.as_str());
    }
    if !split_entities {
        if let Some(attrs) = &query.attrs {
            serializer.append_pair("attrs", &attrs.iter().map(|attr| attr.as_str()).collect::<Vec<_>>().join(","));
        }
        for (key, value) in form_urlencoded::parse(uri.query().unwrap_or("").as_bytes()) {
            serializer.append_pair(&key, &value);
        }
    }
    serializer.finish()
}

/// POST /ngsi-ld/v1/entityMaps -- Create an entity map from query body (section 5.14.4, section 6.34.3.2).
pub async fn create_from_query_post(
    tenant_state::ExistingTenantState(state): EntityMapTenantState,
    OriginalUri(uri): OriginalUri,
    accept: AcceptFormatNoGeo,
    context: ContextUrl,
    prefer: PreferHeader,
    IncomingVia(incoming_via): IncomingVia,
    headers: HeaderMap,
    body: Result<Json<Query>, JsonRejection>,
) -> Result<Response, ProblemResponse> {
    let Json(body) =
        body.map_err(|rejection| classify_json_body(rejection).map(|detail| invalid_request(detail, &uri), |detail| bad_request_data(detail, &uri)))?;
    let context_spec: ContextSpec = context.into_inner();

    let lifetime = body.entity_map_lifetime;
    let query = EntityQueryParameters::try_from(body).with_entity_map_instance(&uri)?;
    query.validate().with_entity_map_instance(&uri)?;
    validate_entity_map_has_filter(&query, &uri)?;
    let local_ids = local_entity_ids(&state, &query, &context_spec, &uri).await?;
    let split_entities = state.forwarding.split_entities(query.split_entities);
    create_map(
        &state,
        MapCreation {
            local_ids,
            lifetime: lifetime.as_ref(),
            kind: EntityMapQueryKind::Entities,
            entity_type: query.entity_type.as_ref(),
            entity_ids: query.id.as_deref(),
            forwarded_query: body_query_string(&query, split_entities, &uri),
            local: query.is_local(),
            resource: "entityMaps",
        },
        &context_spec,
        accept,
        prefer,
        TriggeringRequest {
            via: incoming_via.as_ref(),
            headers: Some(&headers),
        },
        &uri,
    )
    .await
}

/// GET /ngsi-ld/v1/temporal/entityMaps -- Create an entity map from temporal query (section 5.14.5, section 6.35.3.1).
pub async fn create_temporal_from_query(
    tenant_state::ExistingTenantState(state): EntityMapTenantState,
    OriginalUri(uri): OriginalUri,
    accept: AcceptFormatNoGeo,
    context: ContextUrl,
    prefer: PreferHeader,
    IncomingVia(incoming_via): IncomingVia,
    headers: HeaderMap,
    TemporalQuery(params): TemporalQuery,
) -> Result<Response, ProblemResponse> {
    let context: ContextSpec = context.into_inner();
    // Table 6.18.3.2-1: the temporal window, `timeproperty` and `lastN` bound
    // the candidates the same way they bound a temporal query.
    let local_ids = state.temporal.query_ids(&params, &context).await.with_entity_map_instance(&uri)?;
    let split_entities = state.forwarding.split_entities(params.split_entities);
    create_map(
        &state,
        MapCreation {
            local_ids,
            lifetime: params.entity_map_lifetime.as_ref(),
            kind: EntityMapQueryKind::TemporalEntities,
            entity_type: params.entity_type.as_ref(),
            entity_ids: params.id.as_deref(),
            forwarded_query: forwarded_temporal_query_string(&uri, split_entities),
            local: params.is_local(),
            resource: "temporal/entityMaps",
        },
        &context,
        accept,
        prefer,
        TriggeringRequest {
            via: incoming_via.as_ref(),
            headers: Some(&headers),
        },
        &uri,
    )
    .await
}

/// The temporal query string forwarded to each source: with split entities the
/// filters go, but the temporal window stays, as it is the query itself.
fn forwarded_temporal_query_string(uri: &HttpUri, split_entities: bool) -> String {
    let original = uri.query().unwrap_or("");
    if !split_entities {
        return original.to_string();
    }
    let mut serializer = form_urlencoded::Serializer::new(String::new());
    for (key, value) in form_urlencoded::parse(original.as_bytes()) {
        if matches!(
            key.as_ref(),
            "id" | "type" | "idPattern" | "entityMapLifetime" | "timerel" | "timeAt" | "endTimeAt" | "timeproperty" | "lastN"
        ) {
            serializer.append_pair(&key, &value);
        }
    }
    serializer.finish()
}

/// POST /ngsi-ld/v1/temporal/entityMaps -- Create an entity map from temporal query body (section 5.14.5, section 6.35.3.2).
pub async fn create_temporal_from_query_post(
    tenant_state::ExistingTenantState(state): EntityMapTenantState,
    OriginalUri(uri): OriginalUri,
    accept: AcceptFormatNoGeo,
    context: ContextUrl,
    prefer: PreferHeader,
    IncomingVia(incoming_via): IncomingVia,
    headers: HeaderMap,
    body: Result<Json<Query>, JsonRejection>,
) -> Result<Response, ProblemResponse> {
    let Json(body) =
        body.map_err(|rejection| classify_json_body(rejection).map(|detail| invalid_request(detail, &uri), |detail| bad_request_data(detail, &uri)))?;
    let context_spec: ContextSpec = context.into_inner();

    let lifetime = body.entity_map_lifetime;
    let params = TemporalQueryParameters::try_from(body).with_entity_map_instance(&uri)?;
    // Clause 5.14.5.4: a temporal query and one selection criterion are mandatory.
    params.validate(true).with_entity_map_instance(&uri)?;
    let local_ids = state.temporal.query_ids(&params, &context_spec).await.with_entity_map_instance(&uri)?;
    let split_entities = state.forwarding.split_entities(params.split_entities);
    create_map(
        &state,
        MapCreation {
            local_ids,
            lifetime: lifetime.as_ref(),
            kind: EntityMapQueryKind::TemporalEntities,
            entity_type: params.entity_type.as_ref(),
            entity_ids: params.id.as_deref(),
            forwarded_query: temporal_body_query_string(&params, split_entities),
            local: params.is_local(),
            resource: "temporal/entityMaps",
        },
        &context_spec,
        accept,
        prefer,
        TriggeringRequest {
            via: incoming_via.as_ref(),
            headers: Some(&headers),
        },
        &uri,
    )
    .await
}

/// The query string a temporal POST body is forwarded as.
fn temporal_body_query_string(params: &TemporalQueryParameters, split_entities: bool) -> String {
    let mut serializer = form_urlencoded::Serializer::new(String::new());
    if let Some(ids) = &params.id {
        serializer.append_pair("id", &ids.iter().map(Uri::as_str).collect::<Vec<_>>().join(","));
    }
    if let Some(entity_type) = &params.entity_type {
        serializer.append_pair("type", &entity_type.to_string());
    }
    if let Some(pattern) = &params.id_pattern {
        serializer.append_pair("idPattern", pattern.as_str());
    }
    if let Some(timerel) = params.timerel {
        serializer.append_pair("timerel", &timerel.to_string());
    }
    if let Some(time_at) = params.time_at {
        serializer.append_pair("timeAt", &time_at.to_rfc3339());
    }
    if let Some(end_time_at) = params.end_time_at {
        serializer.append_pair("endTimeAt", &end_time_at.to_rfc3339());
    }
    if let Some(timeproperty) = params.timeproperty {
        serializer.append_pair("timeproperty", &timeproperty.to_string());
    }
    if let Some(last_n) = params.last_n {
        serializer.append_pair("lastN", &last_n.to_string());
    }
    if !split_entities && let Some(attrs) = &params.attrs {
        serializer.append_pair("attrs", &attrs.iter().map(|attr| attr.as_str()).collect::<Vec<_>>().join(","));
    }
    serializer.finish()
}

#[cfg(test)]
mod tests {
    use crate::handler::{forwarded_query_string, forwarded_temporal_query_string};
    use axum::http::Uri;

    #[test]
    fn a_split_forward_keeps_the_entity_selectors_and_drops_the_filters() {
        let uri: Uri = "/ngsi-ld/v1/entityMaps?type=Vehicle&q=speed%3E10&attrs=speed&id=urn:a&scopeQ=/Madrid&entityMapLifetime=PT1H"
            .parse()
            .expect("valid URI");
        pretty_assertions::assert_eq!(forwarded_query_string(&uri, true), "type=Vehicle&id=urn%3Aa&entityMapLifetime=PT1H");
        pretty_assertions::assert_eq!(forwarded_query_string(&uri, false), uri.query().expect("query"));
    }

    #[test]
    fn a_split_temporal_forward_keeps_the_window_beside_the_selectors() {
        let uri: Uri = "/ngsi-ld/v1/temporal/entityMaps?type=Vehicle&q=speed%3E10&timerel=after&timeAt=2020-01-01T00:00:00Z&lastN=3"
            .parse()
            .expect("valid URI");
        pretty_assertions::assert_eq!(
            forwarded_temporal_query_string(&uri, true),
            "type=Vehicle&timerel=after&timeAt=2020-01-01T00%3A00%3A00Z&lastN=3"
        );
    }
}
