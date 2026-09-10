//! Assembling the response an entity query answers with (Table 6.4.3.2-3).

use crate::entity::{IntoResponse, NgsiLdCollectionResponse, Response, StatusCode, query_page::QueryPage};
use andromeda_http_headers::{media_type::ResponseMediaType, ngsild_warning::NgsiLdWarning};
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_render::response::RenderedOutput;
use axum::http::Uri as HttpUri;

/// The metadata a query's response carries beside its rendered collection.
pub(crate) struct QueryCollectionMetadata {
    /// Table 6.4.3.2-3 and Table 6.23.3.1-1: 201 when an Entity Map has been
    /// (re)created, 200 otherwise.
    pub(crate) status: StatusCode,
    /// The clause 6.3.17 `NGSILD-Warning` entries the read collected.
    pub(crate) warnings: Vec<NgsiLdWarning>,
    /// The clause 6.3.13 `NGSILD-Results-Count` of the matches, when asked for.
    pub(crate) total_count: Option<i64>,
    /// The Entity Map the query ran under (clause 5.5.14).
    pub(crate) entity_map_uri: Option<Uri>,
    /// How many matching entities the page carried, which decides whether a
    /// following page exists (clause 6.3.10).
    pub(crate) returned: usize,
}

/// Assemble the response around a rendered collection.
pub(crate) fn query_response(
    rendered: RenderedOutput,
    metadata: QueryCollectionMetadata,
    page: &QueryPage,
    media_type: ResponseMediaType,
    instance: &HttpUri,
) -> Response {
    let QueryCollectionMetadata {
        status,
        warnings,
        total_count,
        entity_map_uri,
        returned,
    } = metadata;
    let mut response = NgsiLdCollectionResponse::ok(rendered).with_status(status).with_warnings(warnings);
    if let Some(count) = total_count
        && let Ok(count) = usize::try_from(count)
    {
        response = response.with_total_count(count);
    }
    if let Some(map_uri) = entity_map_uri {
        response = response.with_entity_map_uri(map_uri);
    }
    let mut response = response.into_response();
    page.append_links(&mut response, instance, media_type, total_count, returned);
    response
}
