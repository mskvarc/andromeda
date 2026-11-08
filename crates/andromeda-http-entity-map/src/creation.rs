//! Composing, storing and rendering a created `EntityMap` (clauses 5.14.4.4
//! and 5.14.5.4, Tables 6.34.3.1-1 and 6.35.3.1-1), whichever binding the
//! request came through.

use crate::{error::EntityMapResultExt, state::EntityMapRouteServices};
use andromeda_federation::{
    entity_map::{EntityMapQuery, ForwardEntityMapRequest, ForwardedEntityMap},
    expanded_registration_filters::ExpandedRegistrationFilters,
    outcome::ForwardReadOutcome,
    request::TriggeringRequest,
};
use andromeda_http_context_registry::response_rendering::ResponseContextSource;
use andromeda_http_extraction::{
    accept::AcceptFormatNoGeo,
    api_root::ApiRoot,
    prefer::PreferHeader,
    resource_location::{ResourceCollection, ResourceLocator},
};
use andromeda_http_headers::ngsild_warning::NgsiLdWarning;
use andromeda_http_response::{
    declined_representation::single_source_declined,
    problem_response::ProblemResponse,
    rendered_response::RenderedResponse,
    single::NgsiLdResponse,
};
use andromeda_jsonld::user_context_admission::AdmittedContext;
use andromeda_ngsi_ld_lexicon::{duration::Iso8601Duration, identifier::Identifier};
use andromeda_ngsi_ld_model::{entity_map::EntityMap, entity_map_fingerprint::RequestFingerprint};
use andromeda_ngsi_ld_render::entity_map_body::EntityMapBody;
use axum::{
    http::{StatusCode, Uri as HttpUri},
    response::{IntoResponse, Response},
};
use std::sync::Arc;
use typed_builder::TypedBuilder;

/// What one creation request resolved to.
pub(crate) struct MapCreation<'a> {
    /// The entities this broker holds that match.
    pub(crate) local_ids: Vec<Identifier>,
    /// The requested lifetime, if any.
    pub(crate) lifetime: Option<&'a Iso8601Duration>,
    /// The digest of the creating request's filters (clause 5.5.14).
    pub(crate) fingerprint: RequestFingerprint,
    /// The query the map is created for, which also decides the creation
    /// resource the peers are asked through.
    pub(crate) query: EntityMapQuery<'a>,
    /// The query string forwarded to each source.
    pub(crate) forwarded_query: String,
    /// Whether the request asked for local scope (clause 5.5.13).
    pub(crate) local: bool,
}

/// How a created map is rendered.
#[derive(TypedBuilder)]
pub(crate) struct CreationRendering<'a> {
    /// The request's JSON-LD context, admitted (clause 5.5.7).
    pub(crate) context: &'a AdmittedContext,
    /// The negotiated representation.
    pub(crate) accept: AcceptFormatNoGeo,
    /// The request's `Prefer` header.
    pub(crate) prefer: PreferHeader,
    /// The request URI problem responses name.
    pub(crate) uri: &'a HttpUri,
    /// The `apiRoot` a wrapper the response `Link` names is served under, and
    /// the created map's `Location` sits below.
    pub(crate) api_root: &'a ApiRoot,
}

/// Compose the local map with every map the contributing sources created
/// (clause 5.14.4.4), store it and render it (clauses 6.34.3.1 and 6.35.3.1).
pub(crate) async fn create_map(
    state: &Arc<EntityMapRouteServices>,
    creation: MapCreation<'_>,
    rendering: &CreationRendering<'_>,
    triggering: TriggeringRequest<'_>,
) -> Result<Response, ProblemResponse> {
    let mut map = state
        .entity_maps
        .local_map(creation.local_ids, creation.lifetime, creation.fingerprint)
        .with_entity_map_instance(rendering.uri)?;
    let mut warnings: Vec<NgsiLdWarning> = Vec::new();
    // Clause 5.5.13: local scope forwards nothing.
    if !creation.local {
        let filters = registration_filters(state, creation.query, rendering).await?;
        let forwarded: ForwardReadOutcome<ForwardedEntityMap> = state
            .forwarding
            .forward_entity_map_query(ForwardEntityMapRequest {
                query: creation.query,
                filters: &filters,
                query_string: &creation.forwarded_query,
                context: rendering.context,
                triggering,
            })
            .await
            .with_entity_map_instance(rendering.uri)?;
        // Table 6.34.3.1-1 lists a 501 `ProblemDetails` "used by Registered
        // Context Sources to indicate that the data format of the request is
        // unsupported see clause 6.3.7". The relay is the same one the entity
        // reads make: it applies only where a single exclusive or redirect
        // registration holds all of the data, so a source declining among
        // several stays a clause 6.3.17 warning and the map is still created.
        if let Some(status) = forwarded.sources.single_source_status {
            return Err(single_source_declined(status, rendering.uri));
        }
        for contribution in forwarded.results {
            warnings.extend(contribution.warnings);
            if let Some(remote) = contribution.map {
                map.add_remote(&contribution.registration_id, &remote);
            }
        }
    }
    state.entity_maps.store(&map).await.with_entity_map_instance(rendering.uri)?;
    render_created(&map, creation.query, rendering, state, warnings).await
}

/// The query's context source filter and geoquery expanded under the
/// request's context, each geoquery admitted under the terms of the service
/// that runs its query: the registrations a source map is asked of have to
/// pass them (clauses 4.9 and 5.10.2.4).
///
/// # Errors
///
/// The classified failure of compiling the geoquery or expanding the filter.
async fn registration_filters(
    state: &EntityMapRouteServices,
    query: EntityMapQuery<'_>,
    rendering: &CreationRendering<'_>,
) -> Result<ExpandedRegistrationFilters, ProblemResponse> {
    let (csf, geo, admission) = match query {
        EntityMapQuery::Entities(params) => (params.csf.as_ref(), params.geo.0.as_ref(), state.entity_query.geo_admission()),
        EntityMapQuery::TemporalEntities(params) => (params.csf.as_ref(), params.geo.0.as_ref(), state.temporal.geo_admission()),
    };
    ExpandedRegistrationFilters::expand(state.entity_query.engine(), rendering.context, csf, geo, admission)
        .await
        .with_entity_map_instance(rendering.uri)
}

/// Render a created map: 201 with `Location` and `NGSILD-EntityMap`, or 203
/// with `Preference-Applied` when the body was adapted to an older version
/// (Table 6.34.3.1-1).
async fn render_created(
    map: &EntityMap,
    query: EntityMapQuery<'_>,
    rendering: &CreationRendering<'_>,
    state: &EntityMapRouteServices,
    warnings: Vec<NgsiLdWarning>,
) -> Result<Response, ProblemResponse> {
    let collection = match query {
        EntityMapQuery::Entities(_) => ResourceCollection::EntityMaps,
        EntityMapQuery::TemporalEntities(_) => ResourceCollection::TemporalEntityMaps,
    };
    let body = EntityMapBody::new(map, rendering.prefer.requested_version);
    let renderer = ResponseContextSource::new(state.entity_query.engine(), rendering.context, &state.context_hosting, rendering.api_root)
        .start_rendering_normalized(rendering.accept.media_type(), rendering.prefer.rendering_preferences())
        .await
        .with_entity_map_instance(rendering.uri)?;
    let written = RenderedResponse::resource(renderer.with_body(&body)).with_entity_map_instance(rendering.uri)?;
    let response = NgsiLdResponse::new(StatusCode::CREATED, written)
        .with_entity_map_uri(map.id.clone()) // clone: the header owns its copy because `map` stays borrowed for the Location
        .with_warnings(warnings);
    Ok(match ResourceLocator::from(rendering.api_root).location(collection, &map.id) {
        Some(location) => response.with_location(location),
        None => response,
    }
    .into_response())
}
