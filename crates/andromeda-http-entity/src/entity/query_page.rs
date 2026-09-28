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
    /// [`QueryPage::cut`] once the merge is done. The widening stops one row past
    /// the budget the sources were walked under (clause 4.12): one more is all
    /// it takes to tell that the collection is too large, and reading the rest
    /// could only be discarded.
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
        let merged = forwarded.filter(|results| results.holds_remote_entities());
        let cut_at = if merged.is_some() { PageCut::AfterMerge } else { PageCut::Local };
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
        if let Some(results) = merged {
            query.limit = Some(results.local_row_limit());
            query.offset = Some(0);
        }
        page
    }

    /// Cut the page out of the collection the query answers with, and report
    /// the total alongside it when the request asked to be told.
    ///
    /// `local_total` is what the store counted, which stands whenever the store
    /// held the whole collection. A merged collection is ordered by `id` before
    /// the window is cut from it.
    pub(crate) fn cut(&self, entities: Vec<Entity>, local_total: Option<i64>) -> (Vec<Entity>, Option<i64>) {
        match self.cut_at {
            PageCut::Local => (entities, local_total),
            PageCut::AfterMerge => {
                let total = match self.count {
                    ResultCount::Requested => Some(i64::try_from(entities.len()).unwrap_or(i64::MAX)),
                    ResultCount::NotRequested => None,
                };
                // Clause 4.3.6.7 asks for consistent pagination over the merged
                // collection, which needs one deterministic order: the default
                // `id` order the local store pages in, byte-wise as the store
                // collates identifiers. No other order can reach this point,
                // because clause 5.7.2.4 refuses `orderBy` once a source takes
                // part.
                let mut entities = entities;
                entities.sort_unstable_by(|left, right| left.id.as_str().cmp(right.id.as_str()));
                let entities = entities;
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

#[cfg(test)]
mod tests {
    use crate::entity::query_page::{PageCut, PageMode, QueryPage, ResultCount};
    use andromeda_http_response::pagination::PagePointers;
    use andromeda_ngsi_ld_model::entity::Entity;
    use serde_json::json;

    fn entity(id: &str) -> Entity {
        serde_json::from_value(json!({"id": id, "type": "Vehicle"})).expect("a valid entity")
    }

    fn merged_page(limit: u32, offset: u32, count: ResultCount) -> QueryPage {
        QueryPage {
            limit,
            offset,
            count,
            cut_at: PageCut::AfterMerge,
            pointers: PagePointers::OverRequestUri,
            mode: PageMode::Offset,
        }
    }

    fn ids(entities: &[Entity]) -> Vec<&str> {
        entities.iter().map(|entity| entity.id.as_str()).collect()
    }

    /// Clause 4.3.6.7: the merged collection -- local entities first, then
    /// what each source answered, in arrival order -- is paged in one
    /// deterministic order, the `id` order the local store pages in.
    #[test]
    fn a_merged_collection_is_cut_from_its_identifier_order() {
        let merged = vec![entity("urn:c"), entity("urn:e"), entity("urn:a"), entity("urn:d"), entity("urn:b")];
        let (page, total) = merged_page(2, 1, ResultCount::Requested).cut(merged, None);
        pretty_assertions::assert_eq!(ids(&page), vec!["urn:b", "urn:c"]);
        pretty_assertions::assert_eq!(total, Some(5));
    }

    /// Clause 6.3.13: the count is the size of the whole merged collection,
    /// whatever window is cut from it, and only when it was asked for.
    #[test]
    fn the_count_of_a_merged_collection_is_its_whole_size() {
        let merged = vec![entity("urn:a"), entity("urn:b"), entity("urn:c")];
        let (page, total) = merged_page(10, 3, ResultCount::Requested).cut(merged, Some(1));
        assert!(page.is_empty());
        pretty_assertions::assert_eq!(total, Some(3));
        let (_, total) = merged_page(10, 0, ResultCount::NotRequested).cut(vec![entity("urn:a")], Some(1));
        pretty_assertions::assert_eq!(total, None);
    }

    /// A page the store cut is answered as the store answered it.
    #[test]
    fn a_locally_cut_page_is_left_as_the_store_ordered_it() {
        let page = QueryPage {
            cut_at: PageCut::Local,
            ..merged_page(10, 0, ResultCount::Requested)
        };
        let (entities, total) = page.cut(vec![entity("urn:b"), entity("urn:a")], Some(7));
        pretty_assertions::assert_eq!(ids(&entities), vec!["urn:b", "urn:a"]);
        pretty_assertions::assert_eq!(total, Some(7));
    }
}
