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
//! it apart from the *temporal-window* dimension -- "Temporal representation of
//! resources adds an additional dimension to the pagination" -- which the same
//! clause answers with a 206 and a `Content-Range`. The two are independent:
//! one bounds how many entities the page carries, the other how much history
//! each of them carries, so a response may carry either, both or neither.

use andromeda_http_headers::media_type::ResponseMediaType;
use andromeda_http_response::pagination::{CursorPointers, PageExtent, PageLinks, PagePointers, append_pagination_links};
use andromeda_ngsi_ld_lexicon::uri::Uri as NgsiLdUri;
use andromeda_ngsi_ld_query::cursor::PageCursor;
use andromeda_ngsi_ld_request::query::{SharedQueryParams, temporal::TemporalQueryParameters};
use axum::{http::Uri, response::Response};

/// How the element page points at its neighbouring pages.
enum PageMode {
    /// Offset pointers: `limit`/`offset`, with `first`/`last` when a total is
    /// known (clause 5.5.9.2).
    Offset,
    /// Opaque keyset cursors over the default `id` order (clause 5.5.9.1),
    /// carrying the cursor the page was fetched under (absent on the first
    /// page of the walk).
    Cursor {
        /// The cursor this page was fetched under, `None` on the first page.
        incoming: Option<PageCursor>,
    },
}

/// The window of matching entities a temporal query returned.
pub(crate) struct QueriedPage {
    /// The clause 6.3.10 page size the query resolved to.
    limit: u32,
    /// The clause 6.3.10 page start the query resolved to.
    offset: u32,
    /// Whether the binding the query arrived on can carry a pointer to the
    /// neighbouring pages at all.
    pointers: PagePointers,
    /// How the element page points at its neighbours.
    mode: PageMode,
}

impl QueriedPage {
    /// The page `params` asks for, on a binding whose pointers `pointers`
    /// describes.
    ///
    /// Read before the merge widens the local query: clause 5.5.9 cuts the page
    /// from the merged collection once context sources contribute, which the
    /// local query is opened up for, and the client's own bounds are what the
    /// links point at either way.
    ///
    /// Clause 5.5.9.1: the element page walks by opaque cursor when it continues
    /// a walk (a cursor arrived) or when a fresh, locally-scoped query in the
    /// default `id` order asks for no offset. Clause 4.23.1 forbids a stable
    /// order under distribution, so a cursor is offered only for a local scope,
    /// which is also the scope that never merges a source's history in. Anything
    /// else keeps the offset pointers.
    pub(crate) fn of(params: &TemporalQueryParameters, pointers: PagePointers, incoming: Option<PageCursor>) -> QueriedPage {
        let fresh_eligible = params.ordering.order_by.is_empty() && params.offset.is_none();
        let mode = if params.is_local() && (incoming.is_some() || fresh_eligible) {
            PageMode::Cursor { incoming }
        } else {
            PageMode::Offset
        };
        QueriedPage {
            limit: params.effective_limit(),
            offset: params.effective_offset(),
            pointers,
            mode,
        }
    }

    /// Add the clause 6.3.10 pagination links over this element page.
    ///
    /// In offset mode a query that asked for a count knows its total and drives
    /// `next` from it, plus the Andromeda `first`/`last` pair; one that did not
    /// signals a following page by the page coming back full. In cursor mode the
    /// neighbouring cursors are computed from the page's boundary identifiers and
    /// whether it filled the limit (clause 5.5.9.1), then encoded.
    ///
    /// `returned` counts the matching entities alone. Clause 4.5.23 has a flat
    /// join append the entities the matches link to, and those accompany the
    /// result set rather than belonging to it, so they must not decide whether
    /// a following page exists.
    ///
    /// A binding that carried its query in the request body emits no link:
    /// clause 6.3.10 asks for a target "that could be dereferenced by an
    /// NGSI-LD Client to retrieve the next page", and the clause 5.2.23 body of
    /// clause 6.24.3.1 is not in any URI-reference that could be.
    pub(crate) fn append_links(
        &self,
        response: &mut Response,
        instance: &Uri,
        media_type: ResponseMediaType,
        total: Option<usize>,
        returned: usize,
        boundary: Option<(NgsiLdUri, NgsiLdUri)>,
    ) {
        match self.pointers {
            PagePointers::NotDereferenceable => return,
            PagePointers::OverRequestUri => {}
        }
        let links = match &self.mode {
            PageMode::Offset => {
                let total = total.map(|count| i64::try_from(count).unwrap_or(i64::MAX));
                PageLinks::Offset {
                    offset: i64::from(self.offset),
                    extent: PageExtent::of_page(total, returned),
                }
            }
            PageMode::Cursor { incoming } => {
                // An empty element page has no boundary to page from, so it
                // carries no cursor pointers, matching the offset path's silence
                // on an empty collection.
                let Some((first, last)) = boundary else {
                    return;
                };
                let full = returned >= self.limit as usize;
                let neighbours = PageCursor::neighbours(incoming.as_ref(), first, last, full);
                PageLinks::Cursor(CursorPointers {
                    prev: neighbours.prev.map(|cursor| cursor.encode()),
                    next: neighbours.next.map(|cursor| cursor.encode()),
                    last: Some(neighbours.last.encode()),
                })
            }
        };
        append_pagination_links(response, instance, i64::from(self.limit), links, media_type);
    }
}

#[cfg(test)]
mod tests {
    use crate::temporal::query::page::QueriedPage;
    use andromeda_http_headers::media_type::ResponseMediaType;
    use andromeda_http_response::pagination::PagePointers;
    use andromeda_ngsi_ld_lexicon::uri::Uri as NgsiLdUri;
    use andromeda_ngsi_ld_query::cursor::PageCursor;
    use andromeda_ngsi_ld_request::query::temporal::TemporalQueryParameters;
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

    /// A temporal query bounded to `limit` matches starting at `offset`.
    fn paged_query(limit: Option<u32>, offset: Option<u32>) -> TemporalQueryParameters {
        TemporalQueryParameters {
            limit,
            offset,
            ..TemporalQueryParameters::default()
        }
    }

    fn instance() -> Uri {
        "/ngsi-ld/v1/temporal/entities?type=Vehicle&timerel=before&timeAt=2099-01-01T00:00:00Z&limit=2"
            .parse()
            .expect("a valid request URI")
    }

    /// Clause 4.12: a temporal query shall "provide a mechanism to flag NGSI-LD
    /// Clients when there are remaining NGSI-LD Elements to be traversed", which
    /// clause 6.3.10 binds to `rel="next"`. A page filled to its limit is the
    /// signal a query that did not ask for a count has.
    #[test]
    fn a_full_page_offers_the_next_page() {
        let page = QueriedPage::of(&paged_query(Some(2), Some(0)), PagePointers::OverRequestUri, None);
        let mut response = StatusCode::OK.into_response();

        page.append_links(&mut response, &instance(), ResponseMediaType::JsonLd, None, 2, None);

        let link = links(&response);
        assert!(link.contains("rel=\"next\""), "{link}");
        assert!(link.contains("offset=2"), "{link}");
        assert!(!link.contains("rel=\"prev\""), "{link}");
    }

    /// The last page has nothing left to traverse, so it offers no `next`.
    #[test]
    fn a_short_page_offers_no_next_page() {
        let page = QueriedPage::of(&paged_query(Some(2), Some(0)), PagePointers::OverRequestUri, None);
        let mut response = StatusCode::OK.into_response();

        page.append_links(&mut response, &instance(), ResponseMediaType::JsonLd, None, 1, None);

        pretty_assertions::assert_eq!(links(&response), String::new());
    }

    /// Clause 4.12: a client shall be able to iterate "backwards through a
    /// result set", which clause 6.3.10 binds to `rel="prev"`.
    #[test]
    fn a_page_past_the_start_offers_the_previous_page() {
        let page = QueriedPage::of(&paged_query(Some(2), Some(4)), PagePointers::OverRequestUri, None);
        let mut response = StatusCode::OK.into_response();

        page.append_links(&mut response, &instance(), ResponseMediaType::JsonLd, None, 1, None);

        let link = links(&response);
        assert!(link.contains("rel=\"prev\""), "{link}");
        assert!(link.contains("offset=2"), "{link}");
    }

    /// Clause 6.3.13: a query that asked for a count knows its total, and the
    /// total rather than the fullness of the page decides `next`.
    #[test]
    fn a_counted_query_decides_the_next_page_from_its_total() {
        let page = QueriedPage::of(&paged_query(Some(2), Some(0)), PagePointers::OverRequestUri, None);
        let mut response = StatusCode::OK.into_response();

        page.append_links(&mut response, &instance(), ResponseMediaType::JsonLd, Some(7), 2, None);

        let link = links(&response);
        assert!(link.contains("rel=\"next\""), "{link}");
    }

    /// A counted query whose total is exhausted by the current page is at once the
    /// first and last page: `next`/`prev` are absent and the Andromeda `first`/
    /// `last` would self-point, which clause 5.5.9.1 forbids. It emits no pagination
    /// `Link` header at all.
    #[test]
    fn a_counted_query_whose_total_is_exhausted_offers_no_pagination_links() {
        let page = QueriedPage::of(&paged_query(Some(2), Some(0)), PagePointers::OverRequestUri, None);
        let mut response = StatusCode::OK.into_response();

        page.append_links(&mut response, &instance(), ResponseMediaType::JsonLd, Some(2), 2, None);

        pretty_assertions::assert_eq!(links(&response), String::new());
    }

    /// A counted middle page knows its total and sits past the first page with
    /// more to come, so it carries the whole set: the Andromeda `first`/`last`
    /// pair alongside the clause 6.3.10 `prev`/`next` pointers.
    #[test]
    fn a_counted_middle_page_carries_first_last_prev_and_next() {
        let page = QueriedPage::of(&paged_query(Some(2), Some(2)), PagePointers::OverRequestUri, None);
        let mut response = StatusCode::OK.into_response();

        page.append_links(&mut response, &instance(), ResponseMediaType::JsonLd, Some(7), 2, None);

        let link = links(&response);
        assert!(link.contains("rel=\"first\""), "{link}");
        assert!(link.contains("rel=\"last\""), "{link}");
        assert!(link.contains("rel=\"prev\""), "{link}");
        assert!(link.contains("rel=\"next\""), "{link}");
    }

    /// Clause 6.3.10 asks for a Link Target "that could be dereferenced by an
    /// NGSI-LD Client to retrieve the next page of NGSI-LD Elements", and
    /// clause 5.5.9.1 that the pointers "contain all the parameters needed [...]
    /// without further interactions with the API". The clause 5.2.23 body of
    /// the clause 6.24.3.1 POST binding is in no URI-reference, so that binding
    /// points at nothing rather than at a resource a GET cannot read.
    #[test]
    fn a_body_carried_query_offers_no_pointer_at_all() {
        let page = QueriedPage::of(&paged_query(Some(2), Some(4)), PagePointers::NotDereferenceable, None);
        let mut response = StatusCode::OK.into_response();

        page.append_links(&mut response, &instance(), ResponseMediaType::JsonLd, Some(99), 2, None);

        pretty_assertions::assert_eq!(links(&response), String::new());
    }

    /// Clause 6.3.10: the links carry the negotiated media type, so the client
    /// walks the collection in the representation it asked for.
    #[test]
    fn the_links_carry_the_negotiated_media_type() {
        let page = QueriedPage::of(&paged_query(Some(2), Some(0)), PagePointers::OverRequestUri, None);
        let mut response = StatusCode::OK.into_response();

        page.append_links(&mut response, &instance(), ResponseMediaType::Json, None, 2, None);

        let link = links(&response);
        assert!(link.contains("type=\"application/json\""), "{link}");
    }

    /// A locally-scoped, default-order query with no offset walks the element
    /// dimension by opaque cursor (clause 5.5.9.1).
    fn local_default_query() -> TemporalQueryParameters {
        TemporalQueryParameters {
            limit: Some(2),
            offset: None,
            local: Some(true),
            ..TemporalQueryParameters::default()
        }
    }

    fn element_id(id: &str) -> NgsiLdUri {
        id.parse().expect("valid URI")
    }

    /// Clause 5.5.9.1: the first element page of a local, default-order query
    /// offers opaque cursor pointers -- `next`/`last` carrying a `cursor`, never
    /// an `offset`, and neither `prev` nor `first` (both would name this page).
    #[test]
    fn a_local_default_first_page_offers_opaque_cursor_pointers() {
        let page = QueriedPage::of(&local_default_query(), PagePointers::OverRequestUri, None);
        let mut response = StatusCode::OK.into_response();

        let boundary = Some((element_id("urn:ngsi-ld:E:001"), element_id("urn:ngsi-ld:E:002")));
        page.append_links(&mut response, &instance(), ResponseMediaType::JsonLd, None, 2, boundary);

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
        let page = QueriedPage::of(&local_default_query(), PagePointers::OverRequestUri, incoming);
        let mut response = StatusCode::OK.into_response();

        let boundary = Some((element_id("urn:ngsi-ld:E:003"), element_id("urn:ngsi-ld:E:004")));
        page.append_links(&mut response, &instance(), ResponseMediaType::JsonLd, None, 2, boundary);

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
        let page = QueriedPage::of(&local_default_query(), PagePointers::OverRequestUri, None);
        let mut response = StatusCode::OK.into_response();

        page.append_links(&mut response, &instance(), ResponseMediaType::JsonLd, None, 0, None);

        pretty_assertions::assert_eq!(links(&response), String::new());
    }

    /// A distributed (non-local) query is not eligible for a stable cursor
    /// (clause 4.23.1), so it keeps the offset pointers even with no offset given.
    #[test]
    fn a_distributed_query_keeps_offset_pointers() {
        let params = TemporalQueryParameters {
            limit: Some(2),
            offset: None,
            local: Some(false),
            ..TemporalQueryParameters::default()
        };
        let page = QueriedPage::of(&params, PagePointers::OverRequestUri, None);
        let mut response = StatusCode::OK.into_response();

        let boundary = Some((element_id("urn:ngsi-ld:E:001"), element_id("urn:ngsi-ld:E:002")));
        page.append_links(&mut response, &instance(), ResponseMediaType::JsonLd, None, 2, boundary);

        let link = links(&response);
        assert!(link.contains("rel=\"next\""), "{link}");
        assert!(link.contains("offset="), "{link}");
        assert!(!link.contains("cursor="), "{link}");
    }
}
