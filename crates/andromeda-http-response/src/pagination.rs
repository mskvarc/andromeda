//! The clause 6.3.10 pagination `Link` pointers of every NGSI-LD collection
//! response.
//!
//! This is the one builder of those pointers: entity, temporal, registration,
//! subscription and tenant collections all hand it the window the pagination
//! policy decided and what the page returned. There are two ways a page points
//! at its neighbours. The clause 5.5.9.2 offset pointers carry `limit`/`offset`,
//! plus a `first`/`last` extension when the total is known and the relation
//! does not name the current page. The clause 5.5.9.1 opaque pointers carry a
//! `cursor` token the client only follows. The window decides which. Clause
//! 5.5.9.1 gives the last page "only the pointer to the previous page", and no
//! page points at itself, so in both modes `first` is emitted only on a page
//! with both a previous and a following page, and `last` only on a page with a
//! following one. Every pointer of either family also names the walk it
//! belongs to, as [`PageWalk`] says, which the pagination policy holds the
//! followed request to.

use crate::{
    link_target::{PointedRequest, rebuilt_target},
    page_link_targets::PageLinkTargets,
    page_walk::PageWalk,
};
use andromeda_http_headers::{link::relation_type::RelationType, media_type::ResponseMediaType};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_query::{
    cursor::{NeighbourCursors, PageCursor},
    page_reach::PageReach,
    page_window::{PageStart, PageWindow},
};
use andromeda_ngsi_ld_request::pagination::{
    page_context::{PAGE_CONTEXT_PARAMETER, PageContext},
    posted_reference::POSTED_QUERY_PARAMETER,
    query_digest::{PAGE_QUERY_PARAMETER, QueryDigest},
};
use axum::response::Response;

/// What a page cut by a window returned, as far as its pointers depend on it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageOutcome {
    /// The clause 6.3.13 total of the collection, when it was counted.
    pub total: Option<i64>,
    /// Whether the collection continues past the page in the direction it was
    /// read, as [`PageWindow::settle`] found it before any trim the response
    /// applies afterwards, so a walk continues past every element it scanned.
    pub reach: PageReach,
    /// The first and last identifiers of the page in identifier order, from
    /// which the neighbouring cursors are computed; absent on an empty page.
    pub boundary: Option<(Identifier, Identifier)>,
}

/// The clause 6.3.10 pointers of a page cut by `window`, of the walk `walk`.
///
/// An offset window points by `limit`/`offset` and decides `next` from the
/// total when one is known, and from whether the collection continues past the
/// page otherwise. A cursor window points by the neighbouring cursors of the
/// page's boundary identifiers, decided from whether the collection continues
/// past the page (clause 5.5.9.1). An empty cursor page has no boundary to point
/// from, so it has no pointer, just as an empty offset collection has none;
/// neither has a `limit=0` page, which asked for no element at all.
///
/// Every pointer is `request` with its pagination parameters replaced, naming
/// the walk as `walk` does.
#[must_use]
pub fn page_link_targets(request: &PointedRequest, walk: PageWalk<'_>, window: &PageWindow, outcome: PageOutcome) -> PageLinkTargets {
    let Some(links) = PageLinks::of_window(window, outcome) else {
        return PageLinkTargets::default();
    };
    match walk {
        PageWalk::QueryString { context, query } => {
            let query = query.encode();
            pagination_targets(PageUri::query_string(request, context, &query), window, links)
        }
        PageWalk::PostedQuery(digest) => {
            let digest = digest.encode();
            pagination_targets(PageUri::posted_query(request, &digest), window, links)
        }
    }
}

/// Append the clause 6.3.10 pointers of a page of a query-string walk cut by
/// `window` to `response`, each carrying `context`, the `@context` the page
/// was read under, and each cursor pointer the digest of the query `request`
/// names.
pub fn append_page_links(
    response: &mut Response,
    request: &PointedRequest,
    context: &PageContext,
    window: &PageWindow,
    outcome: PageOutcome,
    media_type: ResponseMediaType,
) {
    let query = QueryDigest::of(request.query());
    page_link_targets(request, PageWalk::QueryString { context, query: &query }, window, outcome).append_to(response, media_type);
}

/// How the presence of a following page is decided.
///
/// A collection with a known total governs `next` by comparing `offset +
/// limit` against that total. A query with no count has no total (computing
/// one on every query is the cost a count-less query avoids), so it signals a
/// following page by the element read past the page (clause 5.5.9.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PageExtent {
    /// A known count of matching resources governs both `prev` and `next`.
    Total(i64),
    /// No total: `next` is emitted while the collection continues past the page.
    Reach(PageReach),
}

/// The already-encoded opaque cursor tokens a keyset-backed page points at
/// (clause 5.5.9.1).
///
/// An absent `prev`/`next` marks a boundary of the collection (clause 5.5.9.1's
/// "previous"/"next" pair): `prev` is absent exactly on the first page, `next`
/// exactly on the last. `last` is the from-end anchor Andromeda adds; the
/// rendered `first` needs both `prev` and `next`, and `last` needs `next`, so
/// the last page carries `prev` alone and no page points at itself.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct CursorPointers {
    /// The opaque token of the previous page, absent on the first page.
    prev: Option<String>,
    /// The opaque token of the following page, absent on the last page.
    next: Option<String>,
    /// The opaque token of the final page, the Andromeda from-end anchor.
    last: Option<String>,
}

/// Which pointer family a collection response carries.
///
/// The two are mutually exclusive per response: a page walks either by
/// `limit`/`offset` (clause 5.5.9.2) or by opaque cursor (clause 5.5.9.1),
/// never both, and the window decides which.
#[derive(Debug, Clone, PartialEq, Eq)]
enum PageLinks {
    /// Classic offset pointers: `limit`/`offset`, with `first`/`last` when the
    /// total is known and the page is not the last one (clause 5.5.9.1).
    Offset {
        /// The page start the request paged from.
        offset: i64,
        /// How the presence of a following page is decided.
        extent: PageExtent,
    },
    /// Opaque keyset cursors: whichever of `first`/`prev`/`next`/`last` apply,
    /// the boundary relations each carrying a `cursor` token in place of an
    /// `offset`, and `first`/`last` emitted only on a page that is not the last.
    Cursor(CursorPointers),
}

impl PageLinks {
    /// The pointers of a page cut by `window` that returned `outcome`.
    fn of_window(window: &PageWindow, outcome: PageOutcome) -> Option<PageLinks> {
        match &window.start {
            PageStart::Offset(offset) => Some(PageLinks::Offset {
                offset: i64::from(*offset),
                extent: outcome.total.map_or(PageExtent::Reach(outcome.reach), PageExtent::Total),
            }),
            PageStart::Cursor(incoming) => {
                let (first, last) = outcome.boundary?;
                Some(PageLinks::Cursor(CursorPointers::from(PageCursor::neighbours(
                    incoming.as_ref(),
                    first,
                    last,
                    outcome.reach,
                ))))
            }
        }
    }
}

// Hand-written because each pointer is the encoded token of its cursor, a
// mapping no derive expresses.
impl From<NeighbourCursors> for CursorPointers {
    /// Encode the neighbouring cursors of a page into the tokens its pointers
    /// carry.
    fn from(neighbours: NeighbourCursors) -> CursorPointers {
        CursorPointers {
            prev: neighbours.prev.map(|cursor| cursor.encode()),
            next: neighbours.next.map(|cursor| cursor.encode()),
            last: Some(neighbours.last.encode()),
        }
    }
}

/// Pagination parameter keys a rebuilt link replaces; every other query-string
/// parameter is preserved so the pointer pages the same query.
const PAGINATION_PARAMS: &[&str] = &[
    "limit",
    "offset",
    "cursor",
    PAGE_CONTEXT_PARAMETER,
    PAGE_QUERY_PARAMETER,
    POSTED_QUERY_PARAMETER,
];

/// The request a page answered, as every pointer of the page is built from it.
#[derive(Debug, Clone, Copy)]
struct PageUri<'request> {
    /// The request the page answered, as a client reaches it.
    request: &'request PointedRequest,
    /// The parameter every pointer carries to name the walk it belongs to.
    walk: (&'static str, &'request str),
    /// The parameter a cursor pointer carries besides, binding the cursor to
    /// the query; absent when [`PageUri::walk`] binds it already.
    cursor_query: Option<(&'static str, &'request str)>,
}

impl<'request> PageUri<'request> {
    /// A page of a query-string walk: every pointer carries `context`, and a
    /// cursor pointer also `query`, the encoded digest of the query.
    fn query_string(request: &'request PointedRequest, context: &'request PageContext, query: &'request str) -> PageUri<'request> {
        PageUri {
            request,
            walk: (PAGE_CONTEXT_PARAMETER, context.as_str()),
            cursor_query: Some((PAGE_QUERY_PARAMETER, query)),
        }
    }

    /// A page of a posted query's walk: every pointer carries `digest`, the
    /// encoded digest of the posted query, which binds a cursor as well.
    const fn posted_query(request: &'request PointedRequest, digest: &'request str) -> PageUri<'request> {
        PageUri {
            request,
            walk: (POSTED_QUERY_PARAMETER, digest),
            cursor_query: None,
        }
    }
}

/// Build a pagination link URL by replacing the pagination parameters in the
/// query string with `set` and the parameter naming the walk, preserving
/// everything else.
///
/// NGSI-LD pagination is `limit`/`offset` (clause 5.5.9.2) or an opaque `cursor`
/// (clause 5.5.9.1), so a rebuilt link replaces exactly those parameters and
/// the ones the walk is held to, and keeps the rest of the query intact.
fn build_page_link(page: PageUri<'_>, set: &[(&str, &str)]) -> String {
    let set: Vec<(&str, &str)> = set.iter().copied().chain([page.walk]).collect();
    rebuilt_target(page.request, PAGINATION_PARAMS, &set)
}

/// A cursor pointer of `page`, carrying `limit` and the cursor `token`.
fn cursor_link(page: PageUri<'_>, limit: &str, token: &str) -> String {
    let set: Vec<(&str, &str)> = [("limit", limit), ("cursor", token)].into_iter().chain(page.cursor_query).collect();
    build_page_link(page, &set)
}

/// The pagination pointers `links` describes over a page of `window`.
///
/// The clause 6.3.10 `prev`/`next` pointers apply as they do today. `first`/`last`
/// are an Andromeda extension over them, which RFC 8288 permits as additional
/// relation types, emitted only when a total (offset mode) or the keyset boundary
/// cursors (cursor mode) are known. Clause 5.5.9.1 gives the last page "only the
/// pointer to the previous page", and a page never points at itself, so `first`
/// appears only on a page with both a previous and a following page, `last` only
/// on a page with a following one, and a single page carries neither. Every
/// pointer carries the window's own `limit`, so a followed link
/// pages by the size the page was cut by.
fn pagination_targets(uri: PageUri<'_>, window: &PageWindow, links: PageLinks) -> PageLinkTargets {
    let limit = i64::from(window.limit);
    if limit <= 0 {
        return PageLinkTargets::default();
    }
    PageLinkTargets::new(match links {
        PageLinks::Offset { offset, extent } => offset_targets(uri, limit, offset, extent),
        PageLinks::Cursor(cursors) => cursor_targets(uri, limit, &cursors),
    })
}

/// The offset-mode pointer targets: `first`/`last` when a total is known, and
/// `prev`/`next` as clause 6.3.10 applies.
fn offset_targets(uri: PageUri<'_>, limit: i64, offset: i64, extent: PageExtent) -> Vec<(RelationType, String)> {
    if matches!(extent, PageExtent::Total(total) if total <= 0) {
        return Vec::new();
    }
    let limit_str = limit.to_string();
    let has_prev = offset > 0;
    let has_next = match extent {
        PageExtent::Total(total) => offset + limit < total,
        PageExtent::Reach(PageReach::Continues) => true,
        PageExtent::Reach(PageReach::Exhausted) => false,
    };

    let mut links: Vec<(RelationType, String)> = Vec::new();
    // `first`/`last` need a known total, so they are emitted only when one is
    // present. They are an Andromeda extension over the clause 6.3.10 `prev`/
    // `next` pointers, which RFC 8288 permits as additional relation types.
    // Clause 5.5.9.1: on the last page "only the pointer to the previous page
    // shall be provided", so both need a following page, and `first` would name
    // the first page itself.
    if let (PageExtent::Total(total), true) = (extent, has_next) {
        let last_offset = ((total - 1) / limit.max(1)) * limit;
        if has_prev {
            links.push((RelationType::First, build_page_link(uri, &[("limit", &limit_str), ("offset", "0")])));
        }
        if last_offset != offset {
            links.push((
                RelationType::Last,
                build_page_link(uri, &[("limit", &limit_str), ("offset", &last_offset.to_string())]),
            ));
        }
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

/// The cursor-mode pointer targets: whichever of `first`/`prev`/`next`/`last`
/// the page offers, each carrying a `cursor`.
///
/// `prev`/`next`/`last` carry an opaque token. `first` carries the empty
/// `cursor=` that starts a walk (clause 5.5.9.1), so a client following it
/// stays in cursor mode whatever the broker's default for a fresh request is.
/// Each binds the cursor to the query, by the digest of its query string or of
/// the posted query.
///
/// The boundary relations track the page's position: `prev` is absent exactly
/// on the first page and `next` exactly on the last (`CursorPointers` mirrors
/// `PageCursor::neighbours`). Clause 5.5.9.1 gives the last page "only the
/// pointer to the previous page", and `first` on the first page would name
/// itself, so `first` needs both `prev` and `next`, and `last` needs `next`.
fn cursor_targets(uri: PageUri<'_>, limit: i64, cursors: &CursorPointers) -> Vec<(RelationType, String)> {
    let limit_str = limit.to_string();
    let mut links: Vec<(RelationType, String)> = Vec::new();
    if cursors.prev.is_some() && cursors.next.is_some() {
        links.push((RelationType::First, cursor_link(uri, &limit_str, "")));
    }
    if let Some(token) = &cursors.prev {
        links.push((RelationType::Prev, cursor_link(uri, &limit_str, token)));
    }
    if let Some(token) = &cursors.next {
        links.push((RelationType::Next, cursor_link(uri, &limit_str, token)));
    }
    if let (true, Some(token)) = (cursors.next.is_some(), &cursors.last) {
        links.push((RelationType::Last, cursor_link(uri, &limit_str, token)));
    }
    links
}

#[cfg(test)]
mod tests {
    use crate::{
        link_target::PointedRequest,
        page_walk::PageWalk,
        pagination::{CursorPointers, PageExtent, PageLinks, PageOutcome, PageUri, append_page_links, page_link_targets, pagination_targets},
    };
    use andromeda_http_headers::{
        link::{header::Link, relation_type::RelationType},
        media_type::ResponseMediaType,
    };
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use andromeda_ngsi_ld_query::{
        cursor::{NeighbourCursors, PageCursor},
        page_reach::PageReach,
        page_window::{PageStart, PageWindow},
    };
    use andromeda_ngsi_ld_request::pagination::{page_context::PageContext, query_digest::QueryDigest};
    use axum::{
        http::{StatusCode, header::LINK},
        response::{IntoResponse, Response},
    };
    use headers::HeaderMapExt;
    use iri_rs::IriBuf;
    use url::Url;

    fn ngsi_ld_uri(value: &str) -> Identifier {
        value.parse().expect("valid URI")
    }

    fn append_pagination_links(response: &mut Response, uri: PageUri<'_>, window: &PageWindow, links: PageLinks, media_type: ResponseMediaType) {
        pagination_targets(uri, window, links).append_to(response, media_type);
    }

    /// A posted query's pointers name it by its digest alone: they carry neither
    /// the `@context` nor a `pageQuery`, which the digest stands for, and a
    /// cursor pointer is bound by the digest as well.
    #[test]
    fn every_pointer_of_a_posted_query_names_it_by_its_digest_alone() {
        let digest = QueryDigest::of(Some("type=Posted"));
        let token = digest.encode();
        let request = pointed("/ngsi-ld/v1/entities?limit=2&count=true");
        let offset_page = page_link_targets(
            &request,
            PageWalk::PostedQuery(&digest),
            &PageWindow::offset(2, 2),
            PageOutcome {
                total: Some(6),
                reach: PageReach::Continues,
                boundary: None,
            },
        );
        let cursor_page = page_link_targets(
            &request,
            PageWalk::PostedQuery(&digest),
            &PageWindow::after(2, ngsi_ld_uri("urn:e:2")),
            PageOutcome {
                total: None,
                reach: PageReach::Continues,
                boundary: Some((ngsi_ld_uri("urn:e:3"), ngsi_ld_uri("urn:e:4"))),
            },
        );
        for (targets, relation) in [
            (&offset_page, RelationType::Next),
            (&offset_page, RelationType::Prev),
            (&cursor_page, RelationType::Next),
            (&cursor_page, RelationType::Prev),
        ] {
            let target = targets.target(&relation).expect("a pointer");
            assert!(target.starts_with("/ngsi-ld/v1/entities?count=true&limit=2&"), "{target}");
            assert!(target.contains(&format!("postedQuery={token}")), "{target}");
            assert!(!target.contains("pageContext") && !target.contains("pageQuery"), "{target}");
        }
        assert!(offset_page.target(&RelationType::Next).is_some_and(|target| target.contains("offset=4")));
        assert!(cursor_page.target(&RelationType::Next).is_some_and(|target| target.contains("cursor=")));
    }

    /// A page pointing nowhere has no pointer to hold a posted query for.
    #[test]
    fn a_single_page_of_a_posted_query_points_nowhere() {
        let digest = QueryDigest::of(Some("type=Posted"));
        let targets = page_link_targets(
            &pointed("/ngsi-ld/v1/entities?limit=2"),
            PageWalk::PostedQuery(&digest),
            &PageWindow::offset(2, 0),
            PageOutcome {
                total: None,
                reach: PageReach::Exhausted,
                boundary: None,
            },
        );
        assert!(targets.is_empty());
    }

    /// The neighbouring cursors of a page are carried as their encoded tokens,
    /// the from-end anchor always among them.
    #[test]
    fn neighbouring_cursors_become_their_encoded_tokens() {
        let neighbours = NeighbourCursors {
            prev: Some(PageCursor::Before(ngsi_ld_uri("urn:e:1"))),
            next: None,
            last: PageCursor::End,
        };
        pretty_assertions::assert_eq!(
            CursorPointers::from(neighbours),
            CursorPointers {
                prev: Some(PageCursor::Before(ngsi_ld_uri("urn:e:1")).encode()),
                next: None,
                last: Some(PageCursor::End.encode()),
            }
        );
    }

    /// An offset window points by offset, deciding `next` from the total.
    #[test]
    fn an_offset_window_points_by_offset() {
        let window = PageWindow {
            limit: 10,
            start: PageStart::Offset(20),
        };
        pretty_assertions::assert_eq!(
            PageLinks::of_window(
                &window,
                PageOutcome {
                    total: Some(45),
                    reach: PageReach::Continues,
                    boundary: None,
                }
            ),
            Some(PageLinks::Offset {
                offset: 20,
                extent: PageExtent::Total(45),
            })
        );
    }

    /// A cursor window points by the cursors around the page's boundary, and a
    /// full first page offers `next` past its last identifier (clause 5.5.9.1).
    #[test]
    fn a_full_cursor_page_points_past_its_last_identifier() {
        let window = PageWindow {
            limit: 2,
            start: PageStart::Cursor(None),
        };
        let boundary = Some((ngsi_ld_uri("urn:e:1"), ngsi_ld_uri("urn:e:2")));
        pretty_assertions::assert_eq!(
            PageLinks::of_window(
                &window,
                PageOutcome {
                    total: None,
                    reach: PageReach::Continues,
                    boundary,
                }
            ),
            Some(PageLinks::Cursor(CursorPointers {
                prev: None,
                next: Some(PageCursor::After(ngsi_ld_uri("urn:e:2")).encode()),
                last: Some(PageCursor::End.encode()),
            }))
        );
    }

    /// Clause 5.5.9.1: a page reached backward that holds the first elements
    /// has no previous elements, so it offers neither `prev` nor `first`, however
    /// full it is.
    #[test]
    fn a_backward_page_holding_the_first_elements_offers_no_prev_or_first() {
        let window = PageWindow {
            limit: 2,
            start: PageStart::Cursor(Some(PageCursor::Before(ngsi_ld_uri("urn:e:3")))),
        };
        let outcome = PageOutcome {
            total: None,
            reach: PageReach::Exhausted,
            boundary: Some((ngsi_ld_uri("urn:e:1"), ngsi_ld_uri("urn:e:2"))),
        };
        let mut response = empty_response();
        append_page_links(
            &mut response,
            &pointed("/entities?limit=2"),
            &core(),
            &window,
            outcome,
            ResponseMediaType::JsonLd,
        );
        let link_str = link_header(&response);
        assert!(!link_str.contains("rel=\"prev\""), "{link_str}");
        assert!(!link_str.contains("rel=\"first\""), "{link_str}");
        assert!(link_str.contains("rel=\"next\""), "{link_str}");
    }

    /// A page reached backward with elements before it offers `prev` back past its first identifier.
    #[test]
    fn a_backward_page_with_elements_before_it_offers_prev() {
        let window = PageWindow {
            limit: 2,
            start: PageStart::Cursor(Some(PageCursor::Before(ngsi_ld_uri("urn:e:5")))),
        };
        let outcome = PageOutcome {
            total: None,
            reach: PageReach::Continues,
            boundary: Some((ngsi_ld_uri("urn:e:3"), ngsi_ld_uri("urn:e:4"))),
        };
        pretty_assertions::assert_eq!(
            PageLinks::of_window(&window, outcome),
            Some(PageLinks::Cursor(CursorPointers {
                prev: Some(PageCursor::Before(ngsi_ld_uri("urn:e:3")).encode()),
                next: Some(PageCursor::After(ngsi_ld_uri("urn:e:4")).encode()),
                last: Some(PageCursor::End.encode()),
            }))
        );
    }

    /// An empty cursor page has no boundary and carries no pointer.
    #[test]
    fn an_empty_cursor_page_carries_no_pointer() {
        let window = PageWindow {
            limit: 2,
            start: PageStart::Cursor(Some(PageCursor::After(ngsi_ld_uri("urn:e:9")))),
        };
        let outcome = PageOutcome {
            total: None,
            reach: PageReach::Exhausted,
            boundary: None,
        };
        pretty_assertions::assert_eq!(PageLinks::of_window(&window, outcome), None);
    }

    /// The digest token the hand-built pointers of these tests carry.
    const DIGEST: &str = "ZGlnZXN0";

    fn core() -> PageContext {
        PageContext::try_from("https://example.com/core.jsonld").expect("an IRI")
    }

    fn empty_response() -> Response {
        StatusCode::OK.into_response()
    }

    /// The request `raw` reached the broker with, under no configured `apiRoot`.
    fn pointed(raw: &str) -> PointedRequest {
        PointedRequest::new(None, raw.parse().expect("valid URI"))
    }

    /// The request `raw` reached the broker with, reached through `api_root`.
    fn pointed_under(api_root: &str, raw: &str) -> PointedRequest {
        PointedRequest::new(Some(Url::parse(api_root).expect("a valid URL")), raw.parse().expect("valid URI"))
    }

    fn offset(offset: i64, extent: PageExtent) -> PageLinks {
        PageLinks::Offset { offset, extent }
    }

    fn link_header(response: &Response) -> String {
        response.headers().get("Link").expect("Link header").to_str().expect("valid string").to_string()
    }

    /// The first page names no `first` (that would self-point, clause 5.5.9.1) and
    /// no `prev`; it carries `last` (a later page exists) and `next`.
    #[test]
    fn counted_first_page_adds_last_and_next_without_first_or_prev() {
        let mut response = empty_response();
        let uri = pointed("/subscriptions?limit=10&offset=0");
        append_pagination_links(
            &mut response,
            PageUri::query_string(&uri, &core(), DIGEST),
            &PageWindow::offset(10, 0),
            offset(0, PageExtent::Total(25)),
            ResponseMediaType::JsonLd,
        );
        let link_str = link_header(&response);
        assert!(!link_str.contains("rel=\"first\""));
        assert!(link_str.contains("rel=\"last\""));
        assert!(link_str.contains("rel=\"next\""));
        assert!(!link_str.contains("rel=\"prev\""));
    }

    #[test]
    fn counted_page_has_first_last_prev_next_past_the_first_page() {
        let mut response = empty_response();
        let uri = pointed("/subscriptions?limit=10&offset=10");
        append_pagination_links(
            &mut response,
            PageUri::query_string(&uri, &core(), DIGEST),
            &PageWindow::offset(10, 0),
            offset(10, PageExtent::Total(25)),
            ResponseMediaType::JsonLd,
        );
        let link_str = link_header(&response);
        assert!(link_str.contains("rel=\"first\""));
        assert!(link_str.contains("rel=\"last\""));
        assert!(link_str.contains("rel=\"prev\""));
        assert!(link_str.contains("rel=\"next\""));
    }

    /// A counted collection that fits in one page is at once the first and last
    /// page, so every relation would name it: `first`/`last` self-point (clause
    /// 5.5.9.1 forbids it) and there is no `prev`/`next`. It emits no pagination
    /// `Link` header at all.
    #[test]
    fn counted_single_page_emits_no_pagination_links() {
        let mut response = empty_response();
        let uri = pointed("/subscriptions");
        append_pagination_links(
            &mut response,
            PageUri::query_string(&uri, &core(), DIGEST),
            &PageWindow::offset(10, 0),
            offset(0, PageExtent::Total(5)),
            ResponseMediaType::JsonLd,
        );
        assert!(response.headers().get("Link").is_none());
    }

    /// NGSI-LD offset pagination is limit/offset only (clause 5.5.9.2): no
    /// offset-mode pagination link ever carries a `page` parameter.
    #[test]
    fn no_offset_pagination_link_carries_a_page_parameter() {
        let mut response = empty_response();
        let uri = pointed("/csourceSubscriptions?limit=10&offset=10");
        append_pagination_links(
            &mut response,
            PageUri::query_string(&uri, &core(), DIGEST),
            &PageWindow::offset(10, 0),
            offset(10, PageExtent::Total(50)),
            ResponseMediaType::JsonLd,
        );
        let link_str = link_header(&response);
        assert!(!link_str.contains("page="), "{link_str}");
        assert!(link_str.contains("offset="), "{link_str}");
        assert!(link_str.contains("rel=\"prev\""), "{link_str}");
    }

    #[test]
    fn offset_full_includes_first_last() {
        let mut response = empty_response();
        let uri = pointed("/csourceRegistrations?limit=10&offset=10");
        append_pagination_links(
            &mut response,
            PageUri::query_string(&uri, &core(), DIGEST),
            &PageWindow::offset(10, 0),
            offset(10, PageExtent::Total(50)),
            ResponseMediaType::JsonLd,
        );
        let link_str = link_header(&response);
        assert!(link_str.contains("rel=\"first\""));
        assert!(link_str.contains("rel=\"last\""));
        assert!(link_str.contains("rel=\"prev\""));
        assert!(link_str.contains("rel=\"next\""));
    }

    #[test]
    fn no_links_when_total_is_zero() {
        let mut response = empty_response();
        let uri = pointed("/subscriptions");
        append_pagination_links(
            &mut response,
            PageUri::query_string(&uri, &core(), DIGEST),
            &PageWindow::offset(10, 0),
            offset(0, PageExtent::Total(0)),
            ResponseMediaType::JsonLd,
        );
        assert!(response.headers().get("Link").is_none());
    }

    #[test]
    fn existing_context_link_header_survives_pagination_links() {
        let mut response = empty_response();
        let context = IriBuf::new("https://example.com/ctx.jsonld".to_string()).expect("an absolute IRI");
        Link::insert_jsonld_context(response.headers_mut(), context);
        let uri = pointed("/subscriptions?limit=1&offset=1");
        append_pagination_links(
            &mut response,
            PageUri::query_string(&uri, &core(), DIGEST),
            &PageWindow::offset(1, 0),
            offset(1, PageExtent::Total(3)),
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
        let uri = pointed("/subscriptions");
        append_pagination_links(
            &mut response,
            PageUri::query_string(&uri, &core(), DIGEST),
            &PageWindow::offset(0, 0),
            offset(0, PageExtent::Total(10)),
            ResponseMediaType::JsonLd,
        );
        assert!(response.headers().get("Link").is_none());
    }

    /// Clause 6.3.10: every pagination link carries a `type` equal to the
    /// negotiated response media type, so it reflects the request's Accept.
    #[test]
    fn link_type_reflects_the_negotiated_media_type() {
        let mut response = empty_response();
        let uri = pointed("/entities?limit=10&offset=0");
        append_pagination_links(
            &mut response,
            PageUri::query_string(&uri, &core(), DIGEST),
            &PageWindow::offset(10, 0),
            offset(0, PageExtent::Total(25)),
            ResponseMediaType::Json,
        );
        let link_str = link_header(&response);
        assert!(link_str.contains("type=\"application/json\""), "{link_str}");
        assert!(!link_str.contains("ld+json"), "{link_str}");
    }

    /// An entity query with no count signals a following page by the element
    /// read past the page, with no total available.
    #[test]
    fn an_uncounted_page_that_continues_offers_next() {
        let mut response = empty_response();
        let uri = pointed("/entities?limit=10&offset=0");
        append_pagination_links(
            &mut response,
            PageUri::query_string(&uri, &core(), DIGEST),
            &PageWindow::offset(10, 0),
            offset(0, PageExtent::Reach(PageReach::Continues)),
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
        let outcome = PageOutcome {
            total: Some(25),
            reach: PageReach::Continues,
            boundary: None,
        };
        pretty_assertions::assert_eq!(
            PageLinks::of_window(&PageWindow::offset(10, 0), outcome),
            Some(offset(0, PageExtent::Total(25)))
        );
    }

    /// A query that did not ask for a count has no total, so whether the
    /// collection continues past the page stands in for one.
    #[test]
    fn an_uncounted_page_reports_its_reach_as_the_extent() {
        let outcome = PageOutcome {
            total: None,
            reach: PageReach::Continues,
            boundary: None,
        };
        pretty_assertions::assert_eq!(
            PageLinks::of_window(&PageWindow::offset(10, 0), outcome),
            Some(offset(0, PageExtent::Reach(PageReach::Continues)))
        );
    }

    /// Clause 5.5.9.1: an uncounted page that exhausted the collection offers no `next`.
    #[test]
    fn an_uncounted_page_that_exhausted_the_collection_offers_no_next() {
        let mut response = empty_response();
        let uri = pointed("/entities?limit=10&offset=10");
        append_pagination_links(
            &mut response,
            PageUri::query_string(&uri, &core(), DIGEST),
            &PageWindow::offset(10, 0),
            offset(10, PageExtent::Reach(PageReach::Exhausted)),
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
        let uri = pointed("/entities?type=Vehicle&limit=10&cursor=AURN");
        let cursors = CursorPointers {
            prev: Some("cHJldg".to_string()),
            next: Some("bmV4dA".to_string()),
            last: Some("ZQ".to_string()),
        };
        append_pagination_links(
            &mut response,
            PageUri::query_string(&uri, &core(), DIGEST),
            &PageWindow::offset(10, 0),
            PageLinks::Cursor(cursors),
            ResponseMediaType::JsonLd,
        );
        let link_str = link_header(&response);
        assert!(link_str.contains("rel=\"first\""), "{link_str}");
        assert!(link_str.contains("rel=\"prev\""), "{link_str}");
        assert!(link_str.contains("rel=\"next\""), "{link_str}");
        assert!(link_str.contains("rel=\"last\""), "{link_str}");
        assert!(link_str.contains("cursor="), "{link_str}");
        assert!(
            link_str.contains("cursor=&") || link_str.contains("cursor=>"),
            "first starts a walk with an empty cursor: {link_str}"
        );
        assert!(!link_str.contains("offset="), "cursor links never carry offset: {link_str}");
        // The incoming cursor is stripped and replaced, never duplicated.
        assert!(!link_str.contains("cursor=AURN"), "the incoming cursor is replaced: {link_str}");
    }

    /// The first page of a cursor walk offers no `prev` and no `first` (both would
    /// name this page, clause 5.5.9.1); it carries `next` and `last`, each a cursor.
    #[test]
    fn cursor_first_page_omits_prev_and_first() {
        let mut response = empty_response();
        let uri = pointed("/entities?type=Vehicle&limit=10");
        let cursors = CursorPointers {
            prev: None,
            next: Some("bmV4dA".to_string()),
            last: Some("ZQ".to_string()),
        };
        append_pagination_links(
            &mut response,
            PageUri::query_string(&uri, &core(), DIGEST),
            &PageWindow::offset(10, 0),
            PageLinks::Cursor(cursors),
            ResponseMediaType::JsonLd,
        );
        let link_str = link_header(&response);
        assert!(!link_str.contains("rel=\"first\""), "{link_str}");
        assert!(!link_str.contains("rel=\"prev\""), "{link_str}");
        assert!(link_str.contains("rel=\"next\""), "{link_str}");
        assert!(link_str.contains("rel=\"last\""), "{link_str}");
    }

    /// Clause 5.5.9.1: the last page of a cursor walk carries only the pointer to
    /// the previous page, so neither `first`, `next` nor `last`.
    #[test]
    fn cursor_last_page_offers_only_prev() {
        let mut response = empty_response();
        let uri = pointed("/entities?type=Vehicle&limit=10&cursor=AURN");
        let cursors = CursorPointers {
            prev: Some("cHJldg".to_string()),
            next: None,
            last: Some("ZQ".to_string()),
        };
        append_pagination_links(
            &mut response,
            PageUri::query_string(&uri, &core(), DIGEST),
            &PageWindow::offset(10, 0),
            PageLinks::Cursor(cursors),
            ResponseMediaType::JsonLd,
        );
        let link_str = link_header(&response);
        assert!(link_str.contains("rel=\"prev\""), "{link_str}");
        assert!(!link_str.contains("rel=\"next\""), "{link_str}");
        assert!(!link_str.contains("rel=\"last\""), "{link_str}");
        assert!(!link_str.contains("rel=\"first\""), "{link_str}");
    }

    /// Clause 5.5.9.1: the last page of a counted offset walk carries only the
    /// pointer to the previous page, so neither `first` nor `last`.
    #[test]
    fn counted_last_page_offers_only_prev() {
        let mut response = empty_response();
        let uri = pointed("/subscriptions?limit=10&offset=20");
        append_pagination_links(
            &mut response,
            PageUri::query_string(&uri, &core(), DIGEST),
            &PageWindow::offset(10, 0),
            offset(20, PageExtent::Total(25)),
            ResponseMediaType::JsonLd,
        );
        let link_str = link_header(&response);
        assert!(link_str.contains("rel=\"prev\""), "{link_str}");
        assert!(!link_str.contains("rel=\"first\""), "{link_str}");
        assert!(!link_str.contains("rel=\"last\""), "{link_str}");
        assert!(!link_str.contains("rel=\"next\""), "{link_str}");
    }

    /// A counted offset page past the end of the collection has no following
    /// page, so it is no different from the last one: only `prev`.
    #[test]
    fn counted_page_past_the_end_offers_only_prev() {
        let mut response = empty_response();
        let uri = pointed("/subscriptions?limit=10&offset=40");
        append_pagination_links(
            &mut response,
            PageUri::query_string(&uri, &core(), DIGEST),
            &PageWindow::offset(10, 0),
            offset(40, PageExtent::Total(25)),
            ResponseMediaType::JsonLd,
        );
        let link_str = link_header(&response);
        assert!(link_str.contains("rel=\"prev\""), "{link_str}");
        assert!(!link_str.contains("rel=\"first\""), "{link_str}");
        assert!(!link_str.contains("rel=\"last\""), "{link_str}");
    }

    /// Cursor-mode links preserve the non-pagination query parameters so the
    /// pointer pages the same query.
    #[test]
    fn cursor_links_preserve_the_query_parameters() {
        let mut response = empty_response();
        let uri = pointed("/entities?type=Vehicle&q=speed%3E10&limit=10");
        let cursors = CursorPointers {
            prev: None,
            next: Some("bmV4dA".to_string()),
            last: Some("ZQ".to_string()),
        };
        append_pagination_links(
            &mut response,
            PageUri::query_string(&uri, &core(), DIGEST),
            &PageWindow::offset(10, 0),
            PageLinks::Cursor(cursors),
            ResponseMediaType::JsonLd,
        );
        let link_str = link_header(&response);
        assert!(link_str.contains("type=Vehicle"), "{link_str}");
        assert!(link_str.contains("speed"), "{link_str}");
    }

    /// The `first` pointer of a cursor page starts a walk with an empty
    /// `cursor=`, so a client following it stays in cursor mode whatever the
    /// broker's default for a fresh request is.
    #[test]
    fn first_link_is_a_cursor_walk_start() {
        let mut response = empty_response();
        let uri = pointed("/entities?type=V&limit=10&cursor=AURN");
        let cursors = CursorPointers {
            prev: Some("cHJldg".to_string()),
            next: Some("bmV4dA".to_string()),
            last: Some("ZQ".to_string()),
        };
        append_pagination_links(
            &mut response,
            PageUri::query_string(&uri, &core(), DIGEST),
            &PageWindow::offset(10, 0),
            PageLinks::Cursor(cursors),
            ResponseMediaType::JsonLd,
        );
        let link_str = link_header(&response);
        assert!(
            link_str.contains("</entities?type=V&limit=10&cursor=&pageQuery=ZGlnZXN0&pageContext="),
            "{link_str}"
        );
    }

    /// Every pointer carries the limit the window was cut by, not the one the
    /// request asked for.
    #[test]
    fn the_link_limit_is_the_window_limit() {
        let mut response = empty_response();
        let uri = pointed("/entities?type=V&limit=5000");
        let window = PageWindow::offset(1000, 0);
        let outcome = PageOutcome {
            total: None,
            reach: PageReach::Continues,
            boundary: None,
        };
        append_page_links(&mut response, &uri, &core(), &window, outcome, ResponseMediaType::JsonLd);
        let link_str = link_header(&response);
        assert!(link_str.contains("limit=1000&offset=1000"), "{link_str}");
        assert!(!link_str.contains("limit=5000"), "{link_str}");
    }

    /// Clause 5.5.9.1: every pointer of an offset page carries the `@context`
    /// the page was read under, replacing whatever the request carried.
    #[test]
    fn every_offset_pointer_carries_the_page_context() {
        let mut response = empty_response();
        let uri = pointed("/entities?type=V&limit=10&offset=10&pageContext=https%3A%2F%2Fexample.com%2Fold.jsonld");
        let context = PageContext::try_from("https://example.com/a.jsonld").expect("an IRI");
        let window = PageWindow::offset(10, 10);
        let outcome = PageOutcome {
            total: Some(50),
            reach: PageReach::Continues,
            boundary: None,
        };
        append_page_links(&mut response, &uri, &context, &window, outcome, ResponseMediaType::JsonLd);
        let link_str = link_header(&response);
        assert_eq!(link_str.matches("pageContext=https%3A%2F%2Fexample.com%2Fa.jsonld").count(), 4, "{link_str}");
        assert!(!link_str.contains("old.jsonld"), "{link_str}");
    }

    /// Clause 5.5.9.1: every pointer of a cursor page carries the `@context`
    /// too, by the same rule.
    #[test]
    fn every_cursor_pointer_carries_the_page_context() {
        let mut response = empty_response();
        let uri = pointed("/entities?type=V&limit=2&cursor=AURN");
        let context = PageContext::try_from("https://example.com/a.jsonld").expect("an IRI");
        let cursors = CursorPointers {
            prev: Some("cHJldg".to_string()),
            next: Some("bmV4dA".to_string()),
            last: Some("ZQ".to_string()),
        };
        append_pagination_links(
            &mut response,
            PageUri::query_string(&uri, &context, DIGEST),
            &PageWindow::offset(2, 0),
            PageLinks::Cursor(cursors),
            ResponseMediaType::JsonLd,
        );
        let link_str = link_header(&response);
        assert_eq!(link_str.matches("pageContext=https%3A%2F%2Fexample.com%2Fa.jsonld").count(), 4, "{link_str}");
    }

    /// Every cursor pointer carries the digest of the query the page answered,
    /// replacing whatever digest the request carried.
    #[test]
    fn every_cursor_pointer_carries_the_digest_of_the_query() {
        let mut response = empty_response();
        let uri = pointed("/entities?type=V&limit=2&cursor=AURN&pageQuery=b2xk");
        let window = PageWindow {
            limit: 2,
            start: PageStart::Cursor(Some(PageCursor::After(ngsi_ld_uri("urn:e:2")))),
        };
        let outcome = PageOutcome {
            total: None,
            reach: PageReach::Continues,
            boundary: Some((ngsi_ld_uri("urn:e:3"), ngsi_ld_uri("urn:e:4"))),
        };
        append_page_links(&mut response, &uri, &core(), &window, outcome, ResponseMediaType::JsonLd);
        let link_str = link_header(&response);
        let digest = format!("pageQuery={}", QueryDigest::of(Some("type=V")).encode());
        assert_eq!(link_str.matches(&digest).count(), 4, "{link_str}");
        assert!(!link_str.contains("b2xk"), "{link_str}");
    }

    /// An offset pointer names a transparent position a client may write
    /// itself (clause 5.5.9.2), so it carries no digest.
    #[test]
    fn an_offset_pointer_carries_no_digest() {
        let mut response = empty_response();
        let uri = pointed("/entities?type=V&limit=10&offset=10");
        let outcome = PageOutcome {
            total: Some(50),
            reach: PageReach::Continues,
            boundary: None,
        };
        append_page_links(&mut response, &uri, &core(), &PageWindow::offset(10, 10), outcome, ResponseMediaType::JsonLd);
        let link_str = link_header(&response);
        assert!(!link_str.contains("pageQuery="), "{link_str}");
    }

    /// The relations a page with neighbours on both sides points by.
    const EVERY_RELATION: [RelationType; 4] = [RelationType::First, RelationType::Prev, RelationType::Next, RelationType::Last];

    /// The target of the `relation` pointer `response` carries.
    fn pointer_target(response: &Response, relation: &RelationType) -> String {
        let link = response.headers().typed_get::<Link>().expect("a Link header");
        let value = link.find_by_rel(relation.clone()).next().expect("a pointer of the relation");
        value.link().as_str().to_owned()
    }

    /// An offset page with neighbours on both sides, so it carries every relation.
    fn offset_page_pointers(request: &PointedRequest) -> Response {
        let mut response = empty_response();
        let outcome = PageOutcome {
            total: Some(10),
            reach: PageReach::Continues,
            boundary: None,
        };
        append_page_links(&mut response, request, &core(), &PageWindow::offset(2, 2), outcome, ResponseMediaType::JsonLd);
        response
    }

    /// A cursor page with neighbours on both sides, so it carries every relation.
    fn cursor_page_pointers(request: &PointedRequest) -> Response {
        let mut response = empty_response();
        let window = PageWindow {
            limit: 2,
            start: PageStart::Cursor(Some(PageCursor::Before(ngsi_ld_uri("urn:e:5")))),
        };
        let outcome = PageOutcome {
            total: None,
            reach: PageReach::Continues,
            boundary: Some((ngsi_ld_uri("urn:e:3"), ngsi_ld_uri("urn:e:4"))),
        };
        append_page_links(&mut response, request, &core(), &window, outcome, ResponseMediaType::JsonLd);
        response
    }

    /// Clause 6.2: a configured `apiRoot` under a prefix puts that prefix before
    /// the path the request reached the broker on, which a proxy serving the
    /// broker under the prefix has stripped.
    #[test]
    fn every_pointer_of_an_offset_page_starts_with_the_configured_api_root_path() {
        let response = offset_page_pointers(&pointed_under("https://proxy.example/context-broker", "/ngsi-ld/v1/entities?type=V&limit=2"));
        for relation in EVERY_RELATION {
            let target = pointer_target(&response, &relation);
            assert!(target.starts_with("/context-broker/ngsi-ld/v1/entities?"), "{relation}: {target}");
        }
    }

    #[test]
    fn every_pointer_of_a_cursor_page_starts_with_the_configured_api_root_path() {
        let response = cursor_page_pointers(&pointed_under("https://proxy.example/context-broker", "/ngsi-ld/v1/entities?type=V&limit=2"));
        for relation in EVERY_RELATION {
            let target = pointer_target(&response, &relation);
            assert!(target.starts_with("/context-broker/ngsi-ld/v1/entities?"), "{relation}: {target}");
        }
    }

    #[test]
    fn every_pointer_under_an_api_root_without_a_path_starts_with_the_request_path() {
        let request = pointed_under("https://proxy.example", "/ngsi-ld/v1/entities?type=V&limit=2");
        for response in [offset_page_pointers(&request), cursor_page_pointers(&request)] {
            for relation in EVERY_RELATION {
                let target = pointer_target(&response, &relation);
                assert!(target.starts_with("/ngsi-ld/v1/entities?"), "{relation}: {target}");
            }
        }
    }

    #[test]
    fn every_pointer_without_a_configured_api_root_starts_with_the_request_path() {
        let request = pointed("/ngsi-ld/v1/entities?type=V&limit=2");
        for response in [offset_page_pointers(&request), cursor_page_pointers(&request)] {
            for relation in EVERY_RELATION {
                let target = pointer_target(&response, &relation);
                assert!(target.starts_with("/ngsi-ld/v1/entities?"), "{relation}: {target}");
            }
        }
    }
}
