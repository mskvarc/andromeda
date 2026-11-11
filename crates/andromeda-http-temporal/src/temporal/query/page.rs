//! The page of matching entities a temporal query answers with.
//!
//! Clause 4.12 lists "Query Temporal Evolution of Entities (clause 5.7.4)"
//! among the operations that shall "provide a mechanism to flag NGSI-LD
//! Clients when there are remaining NGSI-LD Elements to be traversed as part of
//! a result set" and shall "allow NGSI-LD Clients iterating forwards and
//! backwards through a result set". Clause 6.3.10 binds both to the `Link`
//! header, with relation types `next` and `prev` over the `limit` and `offset`
//! parameters.
//!
//! This is the *element* dimension of temporal pagination. Clause 6.3.10 keeps
//! it apart from the *temporal-window* dimension, "Temporal representation of
//! resources adds an additional dimension to the pagination", which the same
//! clause answers with a 206 and a `Content-Range`. The two are independent:
//! one bounds how many entities the page carries, the other how much history
//! each of them carries, so a response may carry either, both or neither.

use crate::temporal::query::forwarding::ForwardedHistory;
use andromeda_http_extraction::pagination_policy::RequestPagination;
use andromeda_http_response::{page_link_targets::PageLinkTargets, page_pointing::PagePointing, pagination::PageOutcome};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_query::{count_policy::CountPolicy, page_reach::PageReach, page_window::PageWindow};
use andromeda_ngsi_ld_request::{
    error::ParameterError,
    pagination::{
        page_pointers::PagePointers,
        page_request::{CollectionOrder, PageRequest},
    },
    query::{QUERY_PAGE_SIZE, temporal::TemporalQueryParameters},
};
use std::borrow::Cow;

/// The window of matching entities a temporal query answers with.
pub(crate) struct QueriedPage {
    /// The clause 6.3.10 page size and where the element page starts, as the
    /// pagination policy decided them.
    window: PageWindow,
    /// Whether the total is reported (clause 6.3.13 `count`).
    count: CountPolicy,
    /// The walk the page's pointers belong to and the request they rebuild.
    pointing: PagePointing,
}

/// What the local temporal store reads for one query.
pub(crate) struct LocalRead<'page> {
    /// The element window the store reads.
    pub(crate) window: Cow<'page, PageWindow>,
    /// Whether the store counts its matches.
    pub(crate) count: CountPolicy,
    /// The client's window, withheld until the merge when context sources
    /// contributed entities, and cut from the merged collection then.
    pub(crate) merged: Option<&'page PageWindow>,
}

impl QueriedPage {
    /// The element page `window` selects, counted as `count` says, pointing at
    /// its neighbours as `pointing` says.
    pub(crate) const fn of(window: PageWindow, count: CountPolicy, pointing: PagePointing) -> QueriedPage {
        QueriedPage { window, count, pointing }
    }

    /// The element page `params` asks for under `pagination`, on a binding
    /// whose pointers `pointers` describes.
    ///
    /// Decided before anything runs, so a pagination parameter the collection
    /// cannot honour is refused before a single context source is contacted.
    /// It pages the element dimension only, never the temporal window.
    ///
    /// # Errors
    ///
    /// Returns whatever the pagination policy refuses (clauses 5.5.9 and
    /// 6.3.10).
    pub(crate) fn requested(pagination: RequestPagination, params: &TemporalQueryParameters, pointers: PagePointers) -> Result<QueriedPage, ParameterError> {
        let request = PageRequest {
            size: QUERY_PAGE_SIZE,
            order: CollectionOrder::of(&params.ordering),
            count: params.count,
            pointers,
            context: &pagination.context,
            query: &pagination.query,
        };
        let window = pagination.policy.window(&params.page, request)?;
        Ok(QueriedPage::of(
            window,
            params.count,
            PagePointing::new(pointers, pagination.context, pagination.query, pagination.pointed),
        ))
    }

    /// Whether the client asked for the total (clause 6.3.13).
    pub(crate) const fn count(&self) -> CountPolicy {
        self.count
    }

    /// What the local store reads, given what the context sources contributed.
    ///
    /// Clause 5.5.9 cuts the page from the merged collection rather than from
    /// the local rows once context sources contribute, so the local query then
    /// reads from the first row, past no cursor, uncounted, and as many rows as
    /// it takes to tell that the merge exceeds the request's budget (clause
    /// 4.12). Without a merge the store reads the client's window itself.
    pub(crate) fn local_read<'page>(&'page self, forwarded: Option<&ForwardedHistory>) -> LocalRead<'page> {
        match forwarded.filter(|history| history.holds_remote_entities()) {
            Some(history) => LocalRead {
                window: Cow::Owned(PageWindow::offset(history.local_row_limit(), 0)),
                count: CountPolicy::Omit,
                merged: Some(&self.window),
            },
            None => LocalRead {
                window: Cow::Borrowed(&self.window),
                count: self.count,
                merged: None,
            },
        }
    }

    /// The clause 6.3.10 pagination links over this element page.
    ///
    /// In offset mode a query that asked for a count knows its total and drives
    /// `next` from it, plus the Andromeda `first`/`last` pair; one that did not
    /// signals a following page by `reach`. In cursor mode the neighbouring
    /// cursors are computed from the page's boundary identifiers and `reach`
    /// (clause 5.5.9.1), then encoded.
    ///
    /// `reach` is read from the matching entities alone. Clause 4.5.23 has a
    /// flat join append the entities the matches link to, and those accompany
    /// the result set rather than belonging to it, so they must not decide
    /// whether a following page exists.
    pub(crate) fn links(&self, total: Option<usize>, reach: PageReach, boundary: Option<(Identifier, Identifier)>) -> PageLinkTargets {
        let total = total.map(|count| i64::try_from(count).unwrap_or(i64::MAX));
        self.pointing.targets(&self.window, PageOutcome { total, reach, boundary })
    }
}

#[cfg(test)]
mod tests {
    use crate::temporal::query::page::QueriedPage;
    use andromeda_http_headers::media_type::ResponseMediaType;
    use andromeda_http_response::{link_target::PointedRequest, page_pointing::PagePointing};
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use andromeda_ngsi_ld_query::{
        count_policy::CountPolicy,
        cursor::PageCursor,
        page_reach::PageReach,
        page_window::{PageStart, PageWindow},
    };
    use andromeda_ngsi_ld_request::pagination::{page_context::PageContext, page_pointers::PagePointers, page_query::PageQuery};
    use axum::{
        http::{StatusCode, Uri},
        response::{IntoResponse, Response},
    };

    /// The `Link` field values a response carries, joined for inspection.
    fn links(response: &Response) -> String {
        response
            .headers()
            .get_all("link")
            .into_iter()
            .filter_map(|value| value.to_str().ok())
            .collect::<Vec<_>>()
            .join(", ")
    }

    /// The pointing of a page built over `pointers`, rebuilding the request
    /// `pointed`.
    fn pointing(pointers: PagePointers, pointed: Uri) -> PagePointing {
        PagePointing::new(
            pointers,
            PageContext::try_from("https://example.com/core.jsonld").expect("an IRI"),
            PageQuery::of(Some("type=Vehicle")),
            PointedRequest::new(None, pointed),
        )
    }

    /// Append the links of `page` to `response`.
    fn append_links(
        page: &QueriedPage,
        response: &mut Response,
        media_type: ResponseMediaType,
        total: Option<usize>,
        reach: PageReach,
        boundary: Option<(Identifier, Identifier)>,
    ) {
        page.links(total, reach, boundary).append_to(response, media_type);
    }

    /// An offset element page of `limit` matches starting at `offset`.
    fn offset_page(limit: u32, offset: u32, pointers: PagePointers) -> QueriedPage {
        QueriedPage::of(PageWindow::offset(limit, offset), CountPolicy::Omit, pointing(pointers, instance()))
    }

    /// A cursor element page of 2 matches starting at `cursor`.
    fn cursor_page(cursor: Option<PageCursor>) -> QueriedPage {
        QueriedPage::of(
            PageWindow {
                limit: 2,
                start: PageStart::Cursor(cursor),
            },
            CountPolicy::Omit,
            pointing(PagePointers::OverRequestUri, instance()),
        )
    }

    fn instance() -> Uri {
        "/ngsi-ld/v1/temporal/entities?type=Vehicle&timerel=before&timeAt=2099-01-01T00:00:00Z&limit=2"
            .parse()
            .expect("a valid request URI")
    }

    /// Clause 4.12: a temporal query shall "provide a mechanism to flag NGSI-LD
    /// Clients when there are remaining NGSI-LD Elements to be traversed", which
    /// clause 6.3.10 binds to `rel="next"`. The entity read past the page is the
    /// signal a query that did not ask for a count has.
    #[test]
    fn a_page_the_collection_continues_past_offers_the_next_page() {
        let page = offset_page(2, 0, PagePointers::OverRequestUri);
        let mut response = StatusCode::OK.into_response();

        append_links(&page, &mut response, ResponseMediaType::JsonLd, None, PageReach::Continues, None);

        let link = links(&response);
        assert!(link.contains("rel=\"next\""), "{link}");
        assert!(link.contains("offset=2"), "{link}");
        assert!(!link.contains("rel=\"prev\""), "{link}");
    }

    /// The last page has nothing left to traverse, so it offers no `next`.
    #[test]
    fn an_exhausted_page_offers_no_next_page() {
        let page = offset_page(2, 0, PagePointers::OverRequestUri);
        let mut response = StatusCode::OK.into_response();

        append_links(&page, &mut response, ResponseMediaType::JsonLd, None, PageReach::Exhausted, None);

        pretty_assertions::assert_eq!(links(&response), String::new());
    }

    /// Clause 4.12: a client shall be able to iterate "backwards through a
    /// result set", which clause 6.3.10 binds to `rel="prev"`.
    #[test]
    fn a_page_past_the_start_offers_the_previous_page() {
        let page = offset_page(2, 4, PagePointers::OverRequestUri);
        let mut response = StatusCode::OK.into_response();

        append_links(&page, &mut response, ResponseMediaType::JsonLd, None, PageReach::Exhausted, None);

        let link = links(&response);
        assert!(link.contains("rel=\"prev\""), "{link}");
        assert!(link.contains("offset=2"), "{link}");
    }

    /// Clause 6.3.13: a query that asked for a count knows its total, and the
    /// total rather than the element read past the page decides `next`.
    #[test]
    fn a_counted_query_decides_the_next_page_from_its_total() {
        let page = offset_page(2, 0, PagePointers::OverRequestUri);
        let mut response = StatusCode::OK.into_response();

        append_links(&page, &mut response, ResponseMediaType::JsonLd, Some(7), PageReach::Continues, None);

        let link = links(&response);
        assert!(link.contains("rel=\"next\""), "{link}");
    }

    /// A counted query whose total is exhausted by the current page is at once the
    /// first and last page: `next`/`prev` are absent and the Andromeda `first`/
    /// `last` would self-point, which clause 5.5.9.1 forbids. It emits no pagination
    /// `Link` header at all.
    #[test]
    fn a_counted_query_whose_total_is_exhausted_offers_no_pagination_links() {
        let page = offset_page(2, 0, PagePointers::OverRequestUri);
        let mut response = StatusCode::OK.into_response();

        append_links(&page, &mut response, ResponseMediaType::JsonLd, Some(2), PageReach::Continues, None);

        pretty_assertions::assert_eq!(links(&response), String::new());
    }

    /// A counted middle page knows its total and sits past the first page with
    /// more to come, so it carries the whole set: the Andromeda `first`/`last`
    /// pair alongside the clause 6.3.10 `prev`/`next` pointers.
    #[test]
    fn a_counted_middle_page_carries_first_last_prev_and_next() {
        let page = offset_page(2, 2, PagePointers::OverRequestUri);
        let mut response = StatusCode::OK.into_response();

        append_links(&page, &mut response, ResponseMediaType::JsonLd, Some(7), PageReach::Continues, None);

        let link = links(&response);
        assert!(link.contains("rel=\"first\""), "{link}");
        assert!(link.contains("rel=\"last\""), "{link}");
        assert!(link.contains("rel=\"prev\""), "{link}");
        assert!(link.contains("rel=\"next\""), "{link}");
    }

    /// Clause 6.3.10 asks for a Link Target "that could be dereferenced by an
    /// NGSI-LD Client to retrieve the next page of NGSI-LD Elements". The
    /// clause 5.2.23 body of the clause 6.24.3.1 POST binding is in no
    /// URI-reference, so its pointers name the query the broker holds by its
    /// key, on the GET binding of the same query.
    #[test]
    fn a_body_carried_query_points_by_the_key_of_the_held_query() {
        let followed: Uri = "/ngsi-ld/v1/temporal/entities?limit=2".parse().expect("a valid request URI");
        let page = QueriedPage::of(PageWindow::offset(2, 4), CountPolicy::Omit, pointing(PagePointers::OverPostedQuery, followed));
        let mut response = StatusCode::OK.into_response();

        append_links(&page, &mut response, ResponseMediaType::JsonLd, Some(99), PageReach::Continues, None);

        let link = links(&response);
        let key = PageQuery::of(Some("type=Vehicle")).encode();
        assert!(
            link.contains(&format!("/ngsi-ld/v1/temporal/entities?limit=2&offset=6&postedQuery={key}")),
            "{link}"
        );
        assert!(!link.contains("pageContext"), "{link}");
    }

    /// Clause 6.3.10: the links carry the negotiated media type, so the client
    /// walks the collection in the representation it asked for.
    #[test]
    fn the_links_carry_the_negotiated_media_type() {
        let page = offset_page(2, 0, PagePointers::OverRequestUri);
        let mut response = StatusCode::OK.into_response();

        append_links(&page, &mut response, ResponseMediaType::Json, None, PageReach::Continues, None);

        let link = links(&response);
        assert!(link.contains("type=\"application/json\""), "{link}");
    }

    fn element_id(id: &str) -> Identifier {
        id.parse().expect("valid URI")
    }

    /// Clause 5.5.9.1: the first element page of a cursor walk offers opaque
    /// cursor pointers: `next`/`last` carrying a `cursor`, never
    /// an `offset`, and neither `prev` nor `first` (both would name this page).
    #[test]
    fn a_local_default_first_page_offers_opaque_cursor_pointers() {
        let page = cursor_page(None);
        let mut response = StatusCode::OK.into_response();

        let boundary = Some((element_id("urn:ngsi-ld:E:001"), element_id("urn:ngsi-ld:E:002")));
        append_links(&page, &mut response, ResponseMediaType::JsonLd, None, PageReach::Continues, boundary);

        let link = links(&response);
        assert!(!link.contains("rel=\"first\""), "{link}");
        assert!(link.contains("rel=\"next\""), "{link}");
        assert!(link.contains("rel=\"last\""), "{link}");
        assert!(!link.contains("rel=\"prev\""), "{link}");
        assert!(link.contains("cursor="), "{link}");
        assert!(!link.contains("offset="), "{link}");
    }

    /// Continuing a walk with an incoming forward cursor offers both `prev` and,
    /// on a full page, `next`.
    #[test]
    fn a_forward_cursor_page_offers_prev_and_next() {
        let incoming = Some(PageCursor::After(element_id("urn:ngsi-ld:E:002")));
        let page = cursor_page(incoming);
        let mut response = StatusCode::OK.into_response();

        let boundary = Some((element_id("urn:ngsi-ld:E:003"), element_id("urn:ngsi-ld:E:004")));
        append_links(&page, &mut response, ResponseMediaType::JsonLd, None, PageReach::Continues, boundary);

        let link = links(&response);
        assert!(link.contains("rel=\"prev\""), "{link}");
        assert!(link.contains("rel=\"next\""), "{link}");
        assert!(link.contains("cursor="), "{link}");
        assert!(!link.contains("offset="), "{link}");
    }

    /// An empty element page in cursor mode has no boundary and so carries no
    /// pagination links at all.
    #[test]
    fn an_empty_cursor_page_carries_no_links() {
        let page = cursor_page(None);
        let mut response = StatusCode::OK.into_response();

        append_links(&page, &mut response, ResponseMediaType::JsonLd, None, PageReach::Exhausted, None);

        pretty_assertions::assert_eq!(links(&response), String::new());
    }
}
