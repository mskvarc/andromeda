//! Shared pagination link construction for NGSI-LD collection responses.
//!
//! Builds the pagination `Link` headers for every collection endpoint -- entity
//! query, temporal query, subscription, CSR subscription, and CSR registration.
//! There are two ways a page points at its neighbours. The clause 5.5.9.2
//! offset pointers carry `limit`/`offset`, and a `first`/`last` pair whenever the
//! total is known. The clause 5.5.9.1 opaque pointers carry a `cursor` token the
//! client only follows, and are what a stable, keyset-backed walk emits; a
//! handler chooses between the two per request (see
//! `docs/conformance/stable-pagination.md`).

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

/// The already-encoded opaque cursor tokens a keyset-backed page points at
/// (clause 5.5.9.1).
///
/// Each token is a `cursor` value the client passes back by following the link;
/// it is computed and encoded by the handler, which owns the boundary
/// identifiers the store returned. An absent `prev`/`next` marks a boundary of
/// the collection (clause 5.5.9.1's "previous"/"next" pair); `last` is the
/// from-end anchor Andromeda adds.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CursorPointers {
    /// The opaque token of the previous page, absent on the first page.
    pub prev: Option<String>,
    /// The opaque token of the following page, absent on the last page.
    pub next: Option<String>,
    /// The opaque token of the final page, the Andromeda from-end anchor.
    pub last: Option<String>,
}

/// Which pointer family a collection response carries.
///
/// The two are mutually exclusive per response: a page walks either by
/// `limit`/`offset` (clause 5.5.9.2) or by opaque cursor (clause 5.5.9.1), never
/// both, and the handler decides which from the effective order and scope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PageLinks {
    /// Classic offset pointers: `limit`/`offset`, with `first`/`last` when the
    /// total is known.
    Offset {
        /// The page start the request paged from.
        offset: i64,
        /// How the presence of a following page is decided.
        extent: PageExtent,
    },
    /// Opaque keyset cursors: `first` plus whichever of `prev`/`next`/`last`
    /// apply, each carrying a `cursor` token in place of an `offset`.
    Cursor(CursorPointers),
}

/// Pagination parameter keys a rebuilt link replaces; every other query-string
/// parameter is preserved so the pointer pages the same query.
const PAGINATION_PARAMS: &[&str] = &["limit", "offset", "cursor"];

/// Build a pagination link URL by replacing the pagination parameters in the
/// query string with `set` and preserving everything else.
///
/// NGSI-LD pagination is `limit`/`offset` (clause 5.5.9.2) or an opaque `cursor`
/// (clause 5.5.9.1), so a rebuilt link replaces exactly those three parameters
/// and keeps the rest of the query intact.
fn build_page_link(uri: &Uri, set: &[(&str, &str)]) -> String {
    let mut pairs: Vec<(String, String)> = Vec::new();
    if let Some(query) = uri.query() {
        for (k, v) in form_urlencoded::parse(query.as_bytes()) {
            if PAGINATION_PARAMS.contains(&k.as_ref()) {
                continue;
            }
            pairs.push((k.into_owned(), v.into_owned()));
        }
    }
    for (key, value) in set {
        pairs.push(((*key).to_string(), (*value).to_string()));
    }

    let query = form_urlencoded::Serializer::new(String::new()).extend_pairs(pairs).finish();

    if query.is_empty() {
        uri.path().to_string()
    } else {
        format!("{}?{}", uri.path(), query)
    }
}

/// Append pagination `Link` headers to a response.
///
/// In offset mode a `first`/`last` pair is emitted when the total is known, and
/// the clause 6.3.10 `prev`/`next` pointers apply as they do today. In cursor
/// mode `first` is always emitted (the collection start is always reachable),
/// alongside whichever of `prev`/`next`/`last` the page offers. `first`/`last`
/// are an Andromeda extension over the spec's `prev`/`next`, which RFC 8288
/// permits as additional relation types.
pub fn append_pagination_links(response: &mut Response, uri: &Uri, limit: i64, links: PageLinks, media_type: ResponseMediaType) {
    if limit <= 0 {
        return;
    }

    // Clause 6.3.10: every pagination link carries a `type` equal to the
    // negotiated response media type, so the client walks the collection with
    // the same representation it asked for.
    let mt = MediaTypeBuf::from(&media_type.media_type());

    let targets = match links {
        PageLinks::Offset { offset, extent } => offset_targets(uri, limit, offset, extent),
        PageLinks::Cursor(cursors) => cursor_targets(uri, limit, &cursors),
    };

    let values = targets
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

/// The offset-mode pointer targets: `first`/`last` when a total is known, and
/// `prev`/`next` as clause 6.3.10 applies.
fn offset_targets(uri: &Uri, limit: i64, offset: i64, extent: PageExtent) -> Vec<(RelationType, String)> {
    if matches!(extent, PageExtent::Total(total) if total <= 0) {
        return Vec::new();
    }
    let limit_str = limit.to_string();
    let has_prev = offset > 0;
    let has_next = match extent {
        PageExtent::Total(total) => offset + limit < total,
        PageExtent::PageFullness { returned } => returned >= limit,
    };

    let mut links: Vec<(RelationType, String)> = Vec::new();
    // `first`/`last` need a known total, so they are emitted only when one is
    // present. They are an Andromeda extension over the clause 6.3.10 `prev`/
    // `next` pointers, which RFC 8288 permits as additional relation types.
    if let PageExtent::Total(total) = extent {
        let last_offset = ((total - 1) / limit.max(1)) * limit;
        links.push((RelationType::First, build_page_link(uri, &[("limit", &limit_str), ("offset", "0")])));
        links.push((
            RelationType::Last,
            build_page_link(uri, &[("limit", &limit_str), ("offset", &last_offset.to_string())]),
        ));
    }
    if has_prev {
        let prev_offset = (offset - limit).max(0);
        links.push((
            RelationType::Prev,
            build_page_link(uri, &[("limit", &limit_str), ("offset", &prev_offset.to_string())]),
        ));
    }
    if has_next {
        links.push((
            RelationType::Next,
            build_page_link(uri, &[("limit", &limit_str), ("offset", &(offset + limit).to_string())]),
        ));
    }
    links
}

/// The cursor-mode pointer targets: `first` with no cursor (the collection
/// start), plus whichever of `prev`/`next`/`last` the page offers, each carrying
/// its opaque `cursor` token.
fn cursor_targets(uri: &Uri, limit: i64, cursors: &CursorPointers) -> Vec<(RelationType, String)> {
    let limit_str = limit.to_string();
    // `first` carries neither `offset` nor `cursor`: it is the bare query, whose
    // cursor-mode first page starts at the collection's beginning. Emitting no
    // `offset` keeps a client that follows `first` in cursor mode rather than
    // dropping it to the offset path.
    let mut links: Vec<(RelationType, String)> = vec![(RelationType::First, build_page_link(uri, &[("limit", &limit_str)]))];
    if let Some(token) = &cursors.prev {
        links.push((RelationType::Prev, build_page_link(uri, &[("limit", &limit_str), ("cursor", token)])));
    }
    if let Some(token) = &cursors.next {
        links.push((RelationType::Next, build_page_link(uri, &[("limit", &limit_str), ("cursor", token)])));
    }
    if let Some(token) = &cursors.last {
        links.push((RelationType::Last, build_page_link(uri, &[("limit", &limit_str), ("cursor", token)])));
    }
    links
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

    fn offset(offset: i64, extent: PageExtent) -> PageLinks {
        PageLinks::Offset { offset, extent }
    }

    fn link_header(response: &Response) -> String {
        response.headers().get("Link").expect("Link header").to_str().expect("valid string").to_string()
    }

    #[test]
    fn counted_page_adds_first_last_and_next_on_first_page() {
        let mut response = empty_response();
        let uri = parse_uri("/subscriptions?limit=10&offset=0");
        append_pagination_links(&mut response, &uri, 10, offset(0, PageExtent::Total(25)), ResponseMediaType::JsonLd);
        let link_str = link_header(&response);
        assert!(link_str.contains("rel=\"first\""));
        assert!(link_str.contains("rel=\"last\""));
        assert!(link_str.contains("rel=\"next\""));
        assert!(!link_str.contains("rel=\"prev\""));
    }

    #[test]
    fn counted_page_has_first_last_prev_next_past_the_first_page() {
        let mut response = empty_response();
        let uri = parse_uri("/subscriptions?limit=10&offset=10");
        append_pagination_links(&mut response, &uri, 10, offset(10, PageExtent::Total(25)), ResponseMediaType::JsonLd);
        let link_str = link_header(&response);
        assert!(link_str.contains("rel=\"first\""));
        assert!(link_str.contains("rel=\"last\""));
        assert!(link_str.contains("rel=\"prev\""));
        assert!(link_str.contains("rel=\"next\""));
    }

    /// A counted collection that fits in one page still knows its total, so it
    /// carries the `first`/`last` pair (both pointing at the single page) with
    /// no `prev`/`next`.
    #[test]
    fn counted_single_page_emits_first_last_without_prev_or_next() {
        let mut response = empty_response();
        let uri = parse_uri("/subscriptions");
        append_pagination_links(&mut response, &uri, 10, offset(0, PageExtent::Total(5)), ResponseMediaType::JsonLd);
        let link_str = link_header(&response);
        assert!(link_str.contains("rel=\"first\""));
        assert!(link_str.contains("rel=\"last\""));
        assert!(!link_str.contains("rel=\"prev\""));
        assert!(!link_str.contains("rel=\"next\""));
    }

    /// NGSI-LD offset pagination is limit/offset only (clause 5.5.9.2): no
    /// offset-mode pagination link ever carries a `page` parameter.
    #[test]
    fn no_offset_pagination_link_carries_a_page_parameter() {
        let mut response = empty_response();
        let uri = parse_uri("/csourceSubscriptions?limit=10&offset=10");
        append_pagination_links(&mut response, &uri, 10, offset(10, PageExtent::Total(50)), ResponseMediaType::JsonLd);
        let link_str = link_header(&response);
        assert!(!link_str.contains("page="), "{link_str}");
        assert!(link_str.contains("offset="), "{link_str}");
        assert!(link_str.contains("rel=\"prev\""), "{link_str}");
    }

    #[test]
    fn offset_full_includes_first_last() {
        let mut response = empty_response();
        let uri = parse_uri("/csourceRegistrations?limit=10&offset=10");
        append_pagination_links(&mut response, &uri, 10, offset(10, PageExtent::Total(50)), ResponseMediaType::JsonLd);
        let link_str = link_header(&response);
        assert!(link_str.contains("rel=\"first\""));
        assert!(link_str.contains("rel=\"last\""));
        assert!(link_str.contains("rel=\"prev\""));
        assert!(link_str.contains("rel=\"next\""));
    }

    #[test]
    fn no_links_when_total_is_zero() {
        let mut response = empty_response();
        let uri = parse_uri("/subscriptions");
        append_pagination_links(&mut response, &uri, 10, offset(0, PageExtent::Total(0)), ResponseMediaType::JsonLd);
        assert!(response.headers().get("Link").is_none());
    }

    #[test]
    fn existing_context_link_header_survives_pagination_links() {
        let mut response = empty_response();
        let context = IriRefBuf::new("https://example.com/ctx.jsonld".to_string()).expect("IRI reference");
        Link::insert_jsonld_contexts(response.headers_mut(), [context]);
        let uri = parse_uri("/subscriptions?limit=1&offset=1");
        append_pagination_links(&mut response, &uri, 1, offset(1, PageExtent::Total(3)), ResponseMediaType::JsonLd);

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
        append_pagination_links(&mut response, &uri, 0, offset(0, PageExtent::Total(10)), ResponseMediaType::JsonLd);
        assert!(response.headers().get("Link").is_none());
    }

    /// Clause 6.3.10: every pagination link carries a `type` equal to the
    /// negotiated response media type, so it reflects the request's Accept.
    #[test]
    fn link_type_reflects_the_negotiated_media_type() {
        let mut response = empty_response();
        let uri = parse_uri("/entities?limit=10&offset=0");
        append_pagination_links(&mut response, &uri, 10, offset(0, PageExtent::Total(25)), ResponseMediaType::Json);
        let link_str = link_header(&response);
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
            offset(0, PageExtent::PageFullness { returned: 10 }),
            ResponseMediaType::JsonLd,
        );
        let link_str = link_header(&response);
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
            offset(10, PageExtent::PageFullness { returned: 4 }),
            ResponseMediaType::JsonLd,
        );
        let link_str = link_header(&response);
        assert!(link_str.contains("rel=\"prev\""), "{link_str}");
        assert!(!link_str.contains("rel=\"next\""), "{link_str}");
    }

    /// Cursor mode on a mid-collection page emits `first` plus all three of
    /// `prev`/`next`/`last`, each pointer carrying a `cursor` and never an
    /// `offset`.
    #[test]
    fn cursor_mode_emits_first_prev_next_last_with_cursor_not_offset() {
        let mut response = empty_response();
        let uri = parse_uri("/entities?type=Vehicle&limit=10&cursor=AURN");
        let cursors = CursorPointers {
            prev: Some("cHJldg".to_string()),
            next: Some("bmV4dA".to_string()),
            last: Some("ZQ".to_string()),
        };
        append_pagination_links(&mut response, &uri, 10, PageLinks::Cursor(cursors), ResponseMediaType::JsonLd);
        let link_str = link_header(&response);
        assert!(link_str.contains("rel=\"first\""), "{link_str}");
        assert!(link_str.contains("rel=\"prev\""), "{link_str}");
        assert!(link_str.contains("rel=\"next\""), "{link_str}");
        assert!(link_str.contains("rel=\"last\""), "{link_str}");
        assert!(link_str.contains("cursor="), "{link_str}");
        assert!(!link_str.contains("offset="), "cursor links never carry offset: {link_str}");
        // The incoming cursor is stripped and replaced, never duplicated.
        assert!(!link_str.contains("cursor=AURN"), "the incoming cursor is replaced: {link_str}");
    }

    /// The first page of a cursor walk offers no `prev`; its `first` link carries
    /// neither `cursor` nor `offset` (it is the bare query), and `next`/`last`
    /// carry cursors.
    #[test]
    fn cursor_first_page_omits_prev_and_first_link_has_no_cursor() {
        let mut response = empty_response();
        let uri = parse_uri("/entities?type=Vehicle&limit=10");
        let cursors = CursorPointers {
            prev: None,
            next: Some("bmV4dA".to_string()),
            last: Some("ZQ".to_string()),
        };
        append_pagination_links(&mut response, &uri, 10, PageLinks::Cursor(cursors), ResponseMediaType::JsonLd);
        let link_str = link_header(&response);
        assert!(link_str.contains("rel=\"first\""), "{link_str}");
        assert!(!link_str.contains("rel=\"prev\""), "{link_str}");
        assert!(link_str.contains("rel=\"next\""), "{link_str}");
        // The `first` target must carry no cursor: isolate it and check.
        let first = link_str.split(',').find(|value| value.contains("rel=\"first\"")).expect("a first link");
        assert!(!first.contains("cursor="), "first must be the bare query: {first}");
        assert!(!first.contains("offset="), "first must carry no offset: {first}");
    }

    /// The last page of a cursor walk offers no `next`.
    #[test]
    fn cursor_last_page_omits_next() {
        let mut response = empty_response();
        let uri = parse_uri("/entities?type=Vehicle&limit=10&cursor=AURN");
        let cursors = CursorPointers {
            prev: Some("cHJldg".to_string()),
            next: None,
            last: Some("ZQ".to_string()),
        };
        append_pagination_links(&mut response, &uri, 10, PageLinks::Cursor(cursors), ResponseMediaType::JsonLd);
        let link_str = link_header(&response);
        assert!(link_str.contains("rel=\"prev\""), "{link_str}");
        assert!(!link_str.contains("rel=\"next\""), "{link_str}");
        assert!(link_str.contains("rel=\"last\""), "{link_str}");
    }

    /// Cursor-mode links preserve the non-pagination query parameters so the
    /// pointer pages the same query.
    #[test]
    fn cursor_links_preserve_the_query_parameters() {
        let mut response = empty_response();
        let uri = parse_uri("/entities?type=Vehicle&q=speed%3E10&limit=10");
        let cursors = CursorPointers {
            prev: None,
            next: Some("bmV4dA".to_string()),
            last: Some("ZQ".to_string()),
        };
        append_pagination_links(&mut response, &uri, 10, PageLinks::Cursor(cursors), ResponseMediaType::JsonLd);
        let link_str = link_header(&response);
        assert!(link_str.contains("type=Vehicle"), "{link_str}");
        assert!(link_str.contains("speed"), "{link_str}");
    }
}
