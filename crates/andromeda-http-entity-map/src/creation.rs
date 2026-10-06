//! Composing, storing and rendering a created `EntityMap` (clauses 5.14.4.4
//! and 5.14.5.4, Tables 6.34.3.1-1 and 6.35.3.1-1), whichever binding the
//! request came through.

use crate::{error::EntityMapResultExt, state::EntityMapRouteServices};
use andromeda_federation::{
    entity_map::{EntityMapQueryKind, ForwardEntityMapRequest, ForwardedEntityMap},
    outcome::ForwardReadOutcome,
    request::TriggeringRequest,
};
use andromeda_http_context_registry::response_rendering::ResponseContextSource;
use andromeda_http_extraction::{accept::AcceptFormatNoGeo, api_root::ApiRoot, prefer::PreferHeader};
use andromeda_http_headers::ngsild_warning::NgsiLdWarning;
use andromeda_http_response::{
    declined_representation::single_source_declined,
    problem_response::ProblemResponse,
    rendered_response::RenderedResponse,
    single::NgsiLdResponse,
};
use andromeda_jsonld::user_context_admission::AdmittedContext;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::{entity_map::EntityMap, entity_map_fingerprint::RequestFingerprint};
use andromeda_ngsi_ld_query::type_selection::grammar::TypeExprRaw;
use andromeda_ngsi_ld_render::entity_map_body::EntityMapBody;
use axum::{
    http::{StatusCode, Uri as HttpUri},
    response::{IntoResponse, Response},
};
use iso8601_duration::Duration as Iso8601Duration;
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
    /// Which creation resource the peers are asked through.
    pub(crate) kind: EntityMapQueryKind,
    /// Compact entity type for registration matching.
    pub(crate) entity_type: Option<&'a TypeExprRaw>,
    /// Entity identifiers for registration matching.
    pub(crate) entity_ids: Option<&'a [Identifier]>,
    /// The query string forwarded to each source.
    pub(crate) forwarded_query: String,
    /// Whether the request asked for local scope (clause 5.5.13).
    pub(crate) local: bool,
    /// The resource the map is created under, for `Location`.
    pub(crate) resource: &'static str,
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
    /// The `apiRoot` a wrapper the response `Link` names is served under.
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
        let forwarded: ForwardReadOutcome<ForwardedEntityMap> = state
            .forwarding
            .forward_entity_map_query(ForwardEntityMapRequest {
                kind: creation.kind,
                entity_type_compact: creation.entity_type,
                entity_ids: creation.entity_ids,
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
    render_created(&map, creation.resource, rendering, state, warnings).await
}

/// Render a created map: 201 with `Location` and `NGSILD-EntityMap`, or 203
/// with `Preference-Applied` when the body was adapted to an older version
/// (Table 6.34.3.1-1).
async fn render_created(
    map: &EntityMap,
    resource: &str,
    rendering: &CreationRendering<'_>,
    state: &EntityMapRouteServices,
    warnings: Vec<NgsiLdWarning>,
) -> Result<Response, ProblemResponse> {
    let location = format!("/ngsi-ld/v1/{resource}/{}", map.id);
    let body = EntityMapBody::new(map, rendering.prefer.requested_version);
    let renderer = ResponseContextSource::new(state.entity_query.engine(), rendering.context, &state.context_hosting, rendering.api_root)
        .start_rendering_normalized(rendering.accept.media_type(), rendering.prefer.rendering_preferences())
        .await
        .with_entity_map_instance(rendering.uri)?;
    let written = RenderedResponse::resource(renderer.with_body(&body)).with_entity_map_instance(rendering.uri)?;
    Ok(NgsiLdResponse::new(StatusCode::CREATED, written)
        .with_location(location)
        .with_entity_map_uri(map.id.clone()) // clone: the header owns the identifier beside the body
        .with_warnings(warnings)
        .into_response())
}
