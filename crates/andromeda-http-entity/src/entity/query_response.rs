//! Assembling the response an entity query answers with (Table 6.4.3.2-3).

use crate::entity::{IntoResponse, NgsiLdCollectionResponse, Response, StatusCode, query_page::QueryPage};
use andromeda_http_headers::{media_type::ResponseMediaType, ngsild_warning::NgsiLdWarning};
use andromeda_http_response::rendered_response::RenderedResponse;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_query::page_reach::PageReach;

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
    pub(crate) entity_map_uri: Option<Identifier>,
    /// Whether the collection continues past the page in the direction it was
    /// read, which decides the clause 5.5.9.1 pointers of an uncounted page.
    pub(crate) reach: PageReach,
    /// The first and last identifiers of the page as displayed, from which a
    /// cursor pointer is computed (clause 5.5.9.1); absent when the page is
    /// empty.
    pub(crate) boundary: Option<(Identifier, Identifier)>,
}

/// Assemble the response around a rendered collection.
pub(crate) fn query_response(rendered: RenderedResponse, metadata: QueryCollectionMetadata, page: &QueryPage, media_type: ResponseMediaType) -> Response {
    let QueryCollectionMetadata {
        status,
        warnings,
        total_count,
        entity_map_uri,
        reach,
        boundary,
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
    page.append_links(&mut response, media_type, total_count, reach, boundary);
    response
}
