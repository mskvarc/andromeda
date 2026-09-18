//! Shared pagination link construction for NGSI-LD collection responses.
//!
//! Provides common logic for building pagination `Link` headers across
//! subscription, CSR subscription, and CSR query endpoints.

use andromeda_http_headers::{
    link::{header::Link, relation_type::RelationType, value::LinkValue},
    media_type::ResponseMediaType,
};
use axum::{
    http::{Uri, header::LINK},
    response::Response,
};
use iri_rs::IriRefBuf;
use mediatype::MediaTypeBuf;
use url::form_urlencoded;

/// Pagination link style controlling which link relations are included.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaginationStyle {
    /// Only `prev`/`next` links using `limit` and `offset` query parameters.
    OffsetPrevNext,
    /// `first`/`last`/`prev`/`next` links using `limit` and `offset` query parameters.
    OffsetFull,
}

/// How the presence of a following page is decided.
///
/// A collection with a known total — subscriptions, registrations, or an entity
/// query that asked for a count — governs `next` by comparing `offset + limit`
/// against that total. An entity query with no count has no total (computing one
/// on every query is the cost the performance work set out to avoid), so it
/// signals a following page by the page having come back full (clause 6.3.10).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageExtent {
    /// A known count of matching resources governs both `prev` and `next`.
    Total(i64),
    /// No total: `next` is emitted when the returned page filled the limit.
    PageFullness {
        /// How many resources the page returned.
        returned: i64,
    },
}

impl PageExtent {
    /// The extent a page of a collection reports.
    ///
    /// Clause 6.3.10 needs to know whether a following page exists. A query
    /// that asked for a count knows its total and decides from it; one that did
    /// not carries no total, and the page having come back full is what stands
    /// in for it. Both entity and temporal queries make the decision this way,
    /// so it is made once here.
    #[must_use]
    pub fn of_page(total: Option<i64>, returned: usize) -> PageExtent {
        match total {
            Some(count) => PageExtent::Total(count),
            None => PageExtent::PageFullness {
                returned: i64::try_from(returned).unwrap_or(i64::MAX),
            },
        }
    }
}

/// Whether a response can carry the clause 6.3.10 pointers to its neighbouring
/// pages at all.
///
/// Clause 6.3.10 requires of the `next` pointer that "the Link Target shall be
/// a URI-reference that could be dereferenced by an NGSI-LD Client to retrieve
/// the next page of NGSI-LD Elements", and clause 5.5.9.1 that "the pointers to
/// NGSI-LD Elements shall contain all the parameters needed to allow NGSI-LD
/// Clients to retrieve the next and previous page, without further interactions
/// with the API". A query carried in a request body meets neither: the resource
/// it names answers only POST, so dereferencing the pointer cannot work, and
/// the pointer carries `limit` and `offset` without the query they page over.
/// Such a binding emits no pointer rather than one that misleads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PagePointers {
    /// The request URI carries the whole query, so a pointer built from it is
    /// dereferenceable and complete.
    OverRequestUri,
    /// The query reached the broker in the request body, which no URI-reference
    /// can carry. Clause 4.12's requirement to "flag NGSI-LD Clients when there
    /// are remaining NGSI-LD Elements to be traversed" is met on such a binding
    /// by the clause 6.3.13 `NGSILD-Results-Count` a counted query asks for.
    NotDereferenceable,
}

/// Build a pagination link URL by replacing pagination parameters in the query string.
fn build_page_link(uri: &Uri, param1: (&str, i64), param2: (&str, i64)) -> String {
    // NGSI-LD pagination is limit/offset only (clause 5.5.9.2), so a rebuilt
    // link replaces exactly those two parameters and preserves the rest.
    let strip_params: &[&str] = &["limit", "offset"];

    let mut pairs: Vec<(String, String)> = Vec::new();
    if let Some(query) = uri.query() {
        for (k, v) in form_urlencoded::parse(query.as_bytes()) {
            if strip_params.contains(&k.as_ref()) {
                continue;
            }
            pairs.push((k.into_owned(), v.into_owned()));
        }
    }
    pairs.push((param1.0.to_string(), param1.1.to_string()));
    pairs.push((param2.0.to_string(), param2.1.to_string()));

    let query = form_urlencoded::Serializer::new(String::new()).extend_pairs(pairs).finish();

    if query.is_empty() {
        uri.path().to_string()
    } else {
        format!("{}?{}", uri.path(), query)
    }
}

/// Append pagination `Link` headers to a response.
///
/// Supports two pagination styles:
/// - [`PaginationStyle::OffsetPrevNext`]: `prev`/`next` using offset-based parameters.
/// - [`PaginationStyle::OffsetFull`]: `first`/`last`/`prev`/`next` using offset-based parameters.
pub fn append_pagination_links(
    response: &mut Response,
    uri: &Uri,
    limit: i64,
    offset: i64,
    extent: PageExtent,
    style: PaginationStyle,
    media_type: ResponseMediaType,
) {
    if limit <= 0 {
        return;
    }
    if matches!(extent, PageExtent::Total(total) if total <= 0) {
        return;
    }

    // Clause 6.3.10: every pagination link carries a `type` equal to the
    // negotiated response media type, so the client walks the collection with
    // the same representation it asked for.
    let mt = MediaTypeBuf::from(&media_type.media_type());

    let has_prev = offset > 0;
    let has_next = match extent {
        PageExtent::Total(total) => offset + limit < total,
        PageExtent::PageFullness { returned } => returned >= limit,
    };

    let mut links: Vec<(RelationType, String)> = Vec::new();

    match style {
        PaginationStyle::OffsetPrevNext => {
            if has_prev {
                let prev_offset = (offset - limit).max(0);
                links.push((RelationType::Prev, build_page_link(uri, ("limit", limit), ("offset", prev_offset))));
            }
            if has_next {
                links.push((RelationType::Next, build_page_link(uri, ("limit", limit), ("offset", offset + limit))));
            }
        }
        PaginationStyle::OffsetFull => {
            // `first`/`last` need a known total, so they are emitted only when one
            // is present; the full style is only ever used where it is.
            if let PageExtent::Total(total) = extent {
                let last_offset = ((total - 1) / limit) * limit;
                links.push((RelationType::First, build_page_link(uri, ("limit", limit), ("offset", 0))));
                links.push((RelationType::Last, build_page_link(uri, ("limit", limit), ("offset", last_offset))));
            }
            if has_prev {
                let prev_offset = (offset - limit).max(0);
                links.push((RelationType::Prev, build_page_link(uri, ("limit", limit), ("offset", prev_offset))));
            }
            if has_next {
                links.push((RelationType::Next, build_page_link(uri, ("limit", limit), ("offset", offset + limit))));
            }
        }
    }

    let values = links
        .into_iter()
        .filter_map(|(relation, target)| {
            // clone: each link value owns its copy of the negotiated media type.
            IriRefBuf::new(target)
                .ok()
                .map(|target| LinkValue::with_params(target, Some(relation), Some(mt.clone())))
        })
        .collect::<Vec<_>>();
    // Appended rather than inserted: a response rendered with the `@context` in
    // the headers (NGSI-LD 6.3.5) already carries a `Link` header, and replacing
    // it would drop the context the client needs to expand the body.
    if let Some(value) = Link::from_values(values).and_then(|link| link.to_header_value()) {
        response.headers_mut().append(LINK, value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{http::StatusCode, response::IntoResponse};

    fn empty_response() -> Response {
        StatusCode::OK.into_response()
    }

    fn parse_uri(s: &str) -> Uri {
        s.parse().expect("valid URI")
    }

    #[test]
    fn offset_prev_next_adds_next_when_more_pages() {
        let mut response = empty_response();
        let uri = parse_uri("/subscriptions?limit=10&offset=0");
        append_pagination_links(
            &mut response,
            &uri,
            10,
            0,
            PageExtent::Total(25),
            PaginationStyle::OffsetPrevNext,
            ResponseMediaType::JsonLd,
        );
        let link = response.headers().get("Link").expect("Link header");
        let link_str = link.to_str().expect("valid string");
        assert!(link_str.contains("rel=\"next\""));
        assert!(!link_str.contains("rel=\"prev\""));
    }

    #[test]
    fn offset_prev_next_adds_prev_when_not_first_page() {
        let mut response = empty_response();
        let uri = parse_uri("/subscriptions?limit=10&offset=10");
        append_pagination_links(
            &mut response,
            &uri,
            10,
            10,
            PageExtent::Total(25),
            PaginationStyle::OffsetPrevNext,
            ResponseMediaType::JsonLd,
        );
        let link = response.headers().get("Link").expect("Link header");
        let link_str = link.to_str().expect("valid string");
        assert!(link_str.contains("rel=\"prev\""));
        assert!(link_str.contains("rel=\"next\""));
    }

    #[test]
    fn offset_prev_next_no_links_when_single_page() {
        let mut response = empty_response();
        let uri = parse_uri("/subscriptions");
        append_pagination_links(
            &mut response,
            &uri,
            10,
            0,
            PageExtent::Total(5),
            PaginationStyle::OffsetPrevNext,
            ResponseMediaType::JsonLd,
        );
        assert!(response.headers().get("Link").is_none());
    }

    /// NGSI-LD pagination is limit/offset only (clause 5.5.9.2): no pagination
    /// link ever carries a `page` parameter, on any endpoint or style.
    #[test]
    fn no_pagination_link_carries_a_page_parameter() {
        for style in [PaginationStyle::OffsetPrevNext, PaginationStyle::OffsetFull] {
            let mut response = empty_response();
            let uri = parse_uri("/csourceSubscriptions?limit=10&offset=10");
            append_pagination_links(&mut response, &uri, 10, 10, PageExtent::Total(50), style, ResponseMediaType::JsonLd);
            let link = response.headers().get("Link").expect("Link header");
            let link_str = link.to_str().expect("valid string");
            assert!(!link_str.contains("page="), "{link_str}");
            assert!(link_str.contains("offset="), "{link_str}");
            assert!(link_str.contains("rel=\"prev\""), "{link_str}");
        }
    }

    #[test]
    fn offset_full_includes_first_last() {
        let mut response = empty_response();
        let uri = parse_uri("/csourceRegistrations?limit=10&offset=10");
        append_pagination_links(
            &mut response,
            &uri,
            10,
            10,
            PageExtent::Total(50),
            PaginationStyle::OffsetFull,
            ResponseMediaType::JsonLd,
        );
        let link = response.headers().get("Link").expect("Link header");
        let link_str = link.to_str().expect("valid string");
        assert!(link_str.contains("rel=\"first\""));
        assert!(link_str.contains("rel=\"last\""));
        assert!(link_str.contains("rel=\"prev\""));
        assert!(link_str.contains("rel=\"next\""));
    }

    #[test]
    fn no_links_when_total_is_zero() {
        let mut response = empty_response();
        let uri = parse_uri("/subscriptions");
        append_pagination_links(
            &mut response,
            &uri,
            10,
            0,
            PageExtent::Total(0),
            PaginationStyle::OffsetPrevNext,
            ResponseMediaType::JsonLd,
        );
        assert!(response.headers().get("Link").is_none());
    }

    #[test]
    fn existing_context_link_header_survives_pagination_links() {
        let mut response = empty_response();
        let context = IriRefBuf::new("https://example.com/ctx.jsonld".to_string()).expect("IRI reference");
        Link::insert_jsonld_contexts(response.headers_mut(), [context]);
        let uri = parse_uri("/subscriptions?limit=1&offset=1");
        append_pagination_links(
            &mut response,
            &uri,
            1,
            1,
            PageExtent::Total(3),
            PaginationStyle::OffsetPrevNext,
            ResponseMediaType::JsonLd,
        );

        let values = response
            .headers()
            .get_all(LINK)
            .into_iter()
            .filter_map(|value| value.to_str().ok())
            .collect::<Vec<_>>();
        assert_eq!(values.len(), 2);
        assert!(values.iter().any(|value| value.contains("json-ld#context")));
        assert!(values.iter().any(|value| value.contains("rel=\"next\"")));
    }

    #[test]
    fn no_links_when_limit_is_zero() {
        let mut response = empty_response();
        let uri = parse_uri("/subscriptions");
        append_pagination_links(
            &mut response,
            &uri,
            0,
            0,
            PageExtent::Total(10),
            PaginationStyle::OffsetPrevNext,
            ResponseMediaType::JsonLd,
        );
        assert!(response.headers().get("Link").is_none());
    }

    /// Clause 6.3.10: every pagination link carries a `type` equal to the
    /// negotiated response media type, so it reflects the request's Accept.
    #[test]
    fn link_type_reflects_the_negotiated_media_type() {
        let mut response = empty_response();
        let uri = parse_uri("/entities?limit=10&offset=0");
        append_pagination_links(
            &mut response,
            &uri,
            10,
            0,
            PageExtent::Total(25),
            PaginationStyle::OffsetPrevNext,
            ResponseMediaType::Json,
        );
        let link = response.headers().get("Link").expect("Link header");
        let link_str = link.to_str().expect("valid string");
        assert!(link_str.contains("type=\"application/json\""), "{link_str}");
        assert!(!link_str.contains("ld+json"), "{link_str}");
    }

    /// An entity query with no count signals a following page by the page having
    /// come back full (clause 6.3.10), with no total available.
    #[test]
    fn page_fullness_adds_next_when_the_page_is_full() {
        let mut response = empty_response();
        let uri = parse_uri("/entities?limit=10&offset=0");
        append_pagination_links(
            &mut response,
            &uri,
            10,
            0,
            PageExtent::PageFullness { returned: 10 },
            PaginationStyle::OffsetPrevNext,
            ResponseMediaType::JsonLd,
        );
        let link = response.headers().get("Link").expect("Link header");
        let link_str = link.to_str().expect("valid string");
        assert!(link_str.contains("rel=\"next\""), "{link_str}");
        assert!(!link_str.contains("rel=\"prev\""), "{link_str}");
    }

    /// Clause 6.3.13: a query that asked for a count knows its total, so the
    /// total is what decides whether a following page exists.
    #[test]
    fn a_counted_page_reports_its_total_as_the_extent() {
        pretty_assertions::assert_eq!(PageExtent::of_page(Some(25), 10), PageExtent::Total(25));
    }

    /// A query that did not ask for a count has no total, so the fullness of
    /// the page it returned stands in for one (clause 6.3.10).
    #[test]
    fn an_uncounted_page_reports_its_fullness_as_the_extent() {
        pretty_assertions::assert_eq!(PageExtent::of_page(None, 10), PageExtent::PageFullness { returned: 10 });
    }

    /// A short final page under the page-fullness rule offers no `next`.
    #[test]
    fn page_fullness_omits_next_when_the_page_is_short() {
        let mut response = empty_response();
        let uri = parse_uri("/entities?limit=10&offset=10");
        append_pagination_links(
            &mut response,
            &uri,
            10,
            10,
            PageExtent::PageFullness { returned: 4 },
            PaginationStyle::OffsetPrevNext,
            ResponseMediaType::JsonLd,
        );
        let link = response.headers().get("Link").expect("Link header");
        let link_str = link.to_str().expect("valid string");
        assert!(link_str.contains("rel=\"prev\""), "{link_str}");
        assert!(!link_str.contains("rel=\"next\""), "{link_str}");
    }
}
