//! The page an entity query answers with (clause 5.5.9).
//!
//! Where the page is cut depends on who holds the entities. With nothing
//! forwarded, the store already returned the window the request asked for.
//! Once context sources contribute entities of their own, the local rows are
//! no longer the collection being paged: the local query then runs unpaged and
//! the window is cut from the merged collection instead.
//!
//! How the page points at its neighbours depends on the effective order and
//! scope. A locally-scoped query in the default `id` order walks the collection
//! with opaque, keyset-backed cursors (clause 5.5.9.1); everything else keeps
//! the offset pointers (clause 5.5.9.2). See
//! `docs/conformance/stable-pagination.md`.

use crate::entity::query_forwarding::ForwardedQueryResults;
use andromeda_http_headers::media_type::ResponseMediaType;
use andromeda_http_response::pagination::{CursorPointers, PageExtent, PageLinks, PagePointers, append_pagination_links};
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_model::entity::Entity;
use andromeda_ngsi_ld_query::cursor::PageCursor;
use andromeda_ngsi_ld_request::query::{SharedQueryParams, entity::parameters::EntityQueryParameters};
use axum::{http::Uri as HttpUri, response::Response};

/// Whether the response reports the total number of matching entities
/// (Table 6.4.3.2-1 `count`).
#[derive(Clone, Copy)]
enum ResultCount {
    /// The request asked for the total.
    Requested,
    /// It did not, and the broker does not count what it was not asked to.
    NotRequested,
}

/// Which collection the requested window is cut from (clause 5.5.9).
#[derive(Clone, Copy)]
enum PageCut {
    /// The local query returned the window itself.
    Local,
    /// Sources contributed entities, so the window is cut after the merge.
    AfterMerge,
}

/// How the page points at its neighbouring pages.
enum PageMode {
    /// Offset pointers: `limit`/`offset`, with `first`/`last` when a total is
    /// known (clause 5.5.9.2).
    Offset,
    /// Opaque keyset cursors over the default `id` order (clause 5.5.9.1),
    /// carrying the cursor the page was fetched under (absent on the first page
    /// of the walk).
    Cursor {
        /// The cursor this page was fetched under, `None` on the first page.
        incoming: Option<PageCursor>,
    },
}

/// The window an entity query asked for, and where it is cut.
pub(crate) struct QueryPage {
    /// The clause 6.3.10 page size.
    limit: u32,
    /// The clause 6.3.10 page start.
    offset: u32,
    /// Whether the total is reported.
    count: ResultCount,
    /// Which collection the window is cut from.
    cut_at: PageCut,
    /// Whether the binding the query arrived on can carry a pointer to the
    /// neighbouring pages at all.
    pointers: PagePointers,
    /// How the page points at its neighbours.
    mode: PageMode,
}

impl QueryPage {
    /// The page `query` asked for.
    ///
    /// Clause 5.5.9: once sources contribute entities the page is cut from the
    /// merged collection rather than from the local rows, so the local query is
    /// widened to the whole result here and the requested window is applied by
    /// [`QueryPage::cut`] once the merge is done.
    ///
    /// Clause 5.5.9.1: the page walks by opaque cursor when it continues a walk
    /// (a cursor arrived) or when a fresh, locally-scoped query in the default
    /// `id` order asks for no offset -- clause 4.23.1 forbids a stable order
    /// under distribution, so a cursor is never offered once a source could
    /// contribute. Otherwise it keeps the offset pointers.
    pub(crate) fn of(
        query: &mut EntityQueryParameters,
        forwarded: Option<&ForwardedQueryResults>,
        pointers: PagePointers,
        is_local: bool,
        incoming: Option<PageCursor>,
    ) -> QueryPage {
        let cut_at = if forwarded.is_some_and(ForwardedQueryResults::holds_remote_entities) {
            PageCut::AfterMerge
        } else {
            PageCut::Local
        };
        // A continuation carries the cursor; a fresh page is cursor-eligible when
        // it names the default order and no offset. Either way the collection
        // must be the local one, which clause 4.23.1's no-order-under-
        // distribution rule reduces to a page cut locally.
        let fresh_eligible = is_local && query.ordering.order_by.is_empty() && query.offset.is_none();
        let mode = if matches!(cut_at, PageCut::Local) && (incoming.is_some() || fresh_eligible) {
            PageMode::Cursor { incoming }
        } else {
            PageMode::Offset
        };
        let page = QueryPage {
            limit: query.effective_limit(),
            offset: query.effective_offset(),
            count: match query.count {
                Some(true) => ResultCount::Requested,
                Some(false) | None => ResultCount::NotRequested,
            },
            cut_at,
            pointers,
            mode,
        };
        if matches!(page.cut_at, PageCut::AfterMerge) {
            query.limit = Some(u32::MAX);
            query.offset = Some(0);
        }
        page
    }

    /// Cut the page out of the collection the query answers with, and report
    /// the total alongside it when the request asked to be told.
    ///
    /// `local_total` is what the store counted, which stands whenever the store
    /// held the whole collection.
    pub(crate) fn cut(&self, entities: Vec<Entity>, local_total: Option<i64>) -> (Vec<Entity>, Option<i64>) {
        match self.cut_at {
            PageCut::Local => (entities, local_total),
            PageCut::AfterMerge => {
                let total = match self.count {
                    ResultCount::Requested => Some(i64::try_from(entities.len()).unwrap_or(i64::MAX)),
                    ResultCount::NotRequested => None,
                };
                let page = entities
                    .into_iter()
                    .skip(usize::try_from(self.offset).unwrap_or(usize::MAX))
                    .take(usize::try_from(self.limit).unwrap_or(usize::MAX))
                    .collect();
                (page, total)
            }
        }
    }

    /// Add the clause 6.3.10 pagination links over this page.
    ///
    /// In offset mode a query that asked for a count knows its total and drives
    /// `next` from it; one that did not signals a following page by the page
    /// coming back full, and a counted query additionally carries the Andromeda
    /// `first`/`last` pair. In cursor mode the neighbouring cursors are computed
    /// from the page's boundary identifiers and whether it filled the limit
    /// (clause 5.5.9.1), then encoded into the pointers.
    ///
    /// A binding that carried its query in the request body emits no link:
    /// clause 6.3.10 asks for a target "that could be dereferenced by an
    /// NGSI-LD Client to retrieve the next page", and the clause 5.2.23 body of
    /// clause 6.23.3.1 is not in any URI-reference that could be.
    pub(crate) fn append_links(
        &self,
        response: &mut Response,
        instance: &HttpUri,
        media_type: ResponseMediaType,
        total: Option<i64>,
        returned: usize,
        boundary: Option<(Uri, Uri)>,
    ) {
        match self.pointers {
            PagePointers::NotDereferenceable => return,
            PagePointers::OverRequestUri => {}
        }
        let links = match &self.mode {
            PageMode::Offset => PageLinks::Offset {
                offset: i64::from(self.offset),
                extent: PageExtent::of_page(total, returned),
            },
            PageMode::Cursor { incoming } => {
                // An empty page has no boundary to page from, so it carries no
                // cursor pointers, matching the offset path's silence on an empty
                // collection.
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
