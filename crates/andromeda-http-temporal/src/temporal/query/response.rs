//! Rendering the collection a temporal query answers with, and the metadata
//! that travels beside it (Table 6.18.3.2-2).

use crate::temporal::{
    AcceptFormatNoGeo,
    IntoResponse,
    NgsiLdCollectionResponse,
    ProblemResponse,
    Response,
    ResponsePayload,
    ResultInstanceExt,
    StatusCode,
    TemporalSimplifyPayload,
    TemporalSimplifyRenderer,
    query::page::QueriedPage,
};
use andromeda_http_headers::ngsild_warning::NgsiLdWarning;
use andromeda_http_response::content_range::build_content_range;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_render::response::ResponseRenderer;
use andromeda_temporal::service::TemporalRange;
use axum::http::Uri;
use serde_json::Value;

/// Project a temporal collection to its `keyValues` representation.
///
/// # Errors
///
/// The classified failure of the simplifying renderer.
pub(crate) fn simplified_collection(entities: Vec<Value>, instance: &Uri) -> Result<Vec<Value>, ProblemResponse> {
    TemporalSimplifyRenderer::new(TemporalSimplifyPayload::Collection(entities))
        .render()
        .with_instance(instance)?
        .into_collection()
        .with_instance(instance)
}

/// The metadata a temporal query's response carries beside its rendered body.
pub(crate) struct TemporalCollectionMetadata {
    /// Table 6.18.3.2-2: 201 when an Entity Map has been (re)created, 206 when
    /// the local history was cut short, 200 otherwise.
    pub(crate) status: StatusCode,
    /// The clause 6.3.17 `NGSILD-Warning` entries the read collected.
    pub(crate) warnings: Vec<NgsiLdWarning>,
    /// The clause 6.3.13 `NGSILD-Results-Count` of the matches, when asked for.
    pub(crate) total_count: Option<usize>,
    /// The `Content-Range` of a history the local read cut short.
    pub(crate) range: Option<TemporalRange>,
    /// The Entity Map the query ran under (clause 5.5.14).
    pub(crate) entity_map_uri: Option<Identifier>,
    /// The clause 6.3.10 element page the `Link` relations point around.
    pub(crate) page: QueriedPage,
    /// How many matching entities that page carried, counted before a flat
    /// clause 4.5.23 join appended the entities they link to.
    pub(crate) returned: usize,
    /// The first and last identifiers of the matching page, from which a cursor
    /// pointer is computed (clause 5.5.9.1); absent when the page is empty.
    pub(crate) boundary: Option<(Identifier, Identifier)>,
}

/// Render the queried collection and assemble the response around it.
///
/// `rendering` is the renderer the request's media type, `Prefer` tokens,
/// clause 6.3.12 representation and `@context` configured (Table 6.18.3.2-2
/// carries the 203 row and clause 6.3.6 the `Preference-Applied` echo, which
/// `NgsiLdCollectionResponse` turns the rendering's outcome into).
///
/// # Errors
///
/// The classified rendering failure when the negotiated representation cannot
/// be produced.
pub(crate) fn query_response(
    accept: AcceptFormatNoGeo,
    rendering: ResponseRenderer<'_>,
    entities: Vec<Value>,
    metadata: TemporalCollectionMetadata,
    instance: &Uri,
) -> Result<Response, ProblemResponse> {
    let TemporalCollectionMetadata {
        status,
        warnings,
        total_count,
        range,
        entity_map_uri,
        page,
        returned,
        boundary,
    } = metadata;
    let rendered = rendering.with_payload(ResponsePayload::Collection(entities)).render().with_instance(instance)?;
    let mut response = NgsiLdCollectionResponse::ok(rendered).with_status(status).with_warnings(warnings);
    if let Some(count) = total_count {
        response = response.with_total_count(count);
    }
    if let Some(range) = range
        && let Some(content_range) = build_content_range(&range.interval, range.observed, range.direction)
    {
        response = response.with_content_range(content_range);
    }
    if let Some(map_uri) = entity_map_uri {
        response = response.with_entity_map_uri(map_uri);
    }
    // Clause 6.3.10 gives the temporal binding two pagination dimensions: the
    // `Content-Range` above bounds the history each entity carries, while these
    // links bound how many entities the page carries. Neither suppresses the
    // other, so a truncated page still says whether more matches follow.
    let mut response = response.into_response();
    page.append_links(&mut response, instance, accept.media_type(), total_count, returned, boundary);
    Ok(response)
}
