//! The page an entity query answers with (clause 5.5.9).
//!
//! Where the page is cut depends on who holds the entities. With nothing
//! forwarded, the store already returned the window the request asked for.
//! Once context sources contribute entities of their own, the local rows are
//! no longer the collection being paged: the local query then runs unpaged and
//! the window is cut from the merged collection instead.
//!
//! How the page points at its neighbours depends on the request alone. A query
//! in the default `id` order that names no offset walks the collection with
//! opaque, keyset-backed cursors (clause 5.5.9.1), whether the collection is
//! the local one or a merged one; an explicit offset or `orderBy` keeps the
//! offset pointers (clause 5.5.9.2). See `docs/conformance/stable-pagination.md`.

use crate::entity::query_forwarding::ForwardedQueryResults;
use andromeda_http_headers::media_type::ResponseMediaType;
use andromeda_http_response::pagination::{PageLinks, PagePointers, append_pagination_links};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::entity::Entity;
use andromeda_ngsi_ld_query::{
    cursor::PageCursor,
    page_window::{PageStart, PageWindow},
};
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

/// The window an entity query asked for, and where it is cut.
pub(crate) struct QueryPage {
    /// The page size and where the page starts.
    window: PageWindow,
    /// Whether the total is reported.
    count: ResultCount,
    /// Which collection the window is cut from.
    cut_at: PageCut,
    /// Whether the binding the query arrived on can carry a pointer to the
    /// neighbouring pages at all.
    pointers: PagePointers,
}

impl QueryPage {
    /// The page `query` asked for.
    ///
    /// Clause 5.5.9: once sources contribute entities the page is cut from the
    /// merged collection rather than from the local rows, so the local query is
    /// widened to the whole result here, cursor and all, and the requested
    /// window is applied by [`QueryPage::cut`] once the merge is done. The
    /// widening stops one row past the budget the sources were walked under
    /// (clause 4.12): one more is all it takes to tell that the collection is
    /// too large, and reading the rest could only be discarded.
    ///
    /// Clause 5.5.9.1: the page walks by opaque cursor when it continues a walk
    /// (a cursor arrived) or when a fresh query in the default `id` order asks
    /// for no offset. The merged collection is paged in that same `id` order, so
    /// a cursor serves it as well as the local one. Clause 4.23.1's "sort
    /// ordering is never applied to distributed operations" concerns the
    /// clause 4.23 Entity ordering a client requests with `orderBy`, which keeps
    /// the offset pointers.
    pub(crate) fn of(
        query: &mut EntityQueryParameters,
        forwarded: Option<&ForwardedQueryResults>,
        pointers: PagePointers,
        incoming: Option<PageCursor>,
    ) -> QueryPage {
        let merged = forwarded.filter(|results| results.holds_remote_entities());
        let cut_at = if merged.is_some() { PageCut::AfterMerge } else { PageCut::Local };
        let start = match incoming {
            Some(cursor) => PageStart::Cursor(Some(cursor)),
            None if query.ordering.order_by.is_empty() && query.offset.is_none() => PageStart::Cursor(None),
            None => PageStart::Offset(query.effective_offset()),
        };
        let page = QueryPage {
            window: PageWindow {
                limit: query.effective_limit(),
                start,
            },
            count: match query.count {
                Some(true) => ResultCount::Requested,
                Some(false) | None => ResultCount::NotRequested,
            },
            cut_at,
            pointers,
        };
        if let Some(results) = merged {
            query.limit = Some(results.local_row_limit());
            query.offset = Some(0);
            query.cursor = None;
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
                (self.window.cut(entities, |entity| entity.id.as_str()), total)
            }
        }
    }

    /// Add the clause 6.3.10 pagination links over this page.
    ///
    /// `boundary` is the first and last identifier of the page as displayed,
    /// from which the neighbouring cursors of a cursor page are computed.
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
        boundary: Option<(Identifier, Identifier)>,
    ) {
        match self.pointers {
            PagePointers::NotDereferenceable => {}
            PagePointers::OverRequestUri => {
                if let Some(links) = PageLinks::of_window(&self.window, total, returned, boundary) {
                    append_pagination_links(response, instance, i64::from(self.window.limit), links, media_type);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::entity::query_page::{PageCut, QueryPage, ResultCount};
    use andromeda_http_response::pagination::PagePointers;
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use andromeda_ngsi_ld_model::entity::Entity;
    use andromeda_ngsi_ld_query::{
        cursor::PageCursor,
        page_window::{PageStart, PageWindow},
    };
    use andromeda_ngsi_ld_request::query::entity::parameters::EntityQueryParameters;
    use serde_json::json;

    fn entity(id: &str) -> Entity {
        serde_json::from_value(json!({"id": id, "type": "Vehicle"})).expect("a valid entity")
    }

    fn uri(id: &str) -> Identifier {
        id.parse().expect("valid URI")
    }

    fn parameters(query: &str) -> EntityQueryParameters {
        serde_urlencoded::from_str(query).expect("a valid entity query")
    }

    fn merged_page(limit: u32, start: PageStart, count: ResultCount) -> QueryPage {
        QueryPage {
            window: PageWindow { limit, start },
            count,
            cut_at: PageCut::AfterMerge,
            pointers: PagePointers::OverRequestUri,
        }
    }

    fn ids(entities: &[Entity]) -> Vec<&str> {
        entities.iter().map(|entity| entity.id.as_str()).collect()
    }

    fn five_merged() -> Vec<Entity> {
        vec![entity("urn:c"), entity("urn:e"), entity("urn:a"), entity("urn:d"), entity("urn:b")]
    }

    /// Clause 4.3.6.7: the merged collection -- local entities first, then
    /// what each source answered, in arrival order -- is paged in one
    /// deterministic order, the `id` order the local store pages in.
    #[test]
    fn a_merged_collection_is_cut_from_its_identifier_order() {
        let (page, total) = merged_page(2, PageStart::Offset(1), ResultCount::Requested).cut(five_merged(), None);
        pretty_assertions::assert_eq!(ids(&page), vec!["urn:b", "urn:c"]);
        pretty_assertions::assert_eq!(total, Some(5));
    }

    /// Clause 5.5.9.1: a cursor continues a walk over the merged collection
    /// from its boundary, in the same identifier order the offset cut uses,
    /// and the count stays the size of the whole collection (clause 6.3.13).
    #[test]
    fn a_cursor_continues_a_walk_over_the_merged_collection() {
        let start = PageStart::Cursor(Some(PageCursor::After(uri("urn:b"))));
        let (page, total) = merged_page(2, start, ResultCount::Requested).cut(five_merged(), None);
        pretty_assertions::assert_eq!(ids(&page), vec!["urn:c", "urn:d"]);
        pretty_assertions::assert_eq!(total, Some(5));
    }

    /// A backward cursor over the merged collection returns the page before its
    /// boundary, in ascending order.
    #[test]
    fn a_backward_cursor_pages_back_through_the_merged_collection() {
        let start = PageStart::Cursor(Some(PageCursor::Before(uri("urn:d"))));
        let (page, _) = merged_page(2, start, ResultCount::NotRequested).cut(five_merged(), None);
        pretty_assertions::assert_eq!(ids(&page), vec!["urn:b", "urn:c"]);
    }

    /// Clause 6.3.13: the count of a merged collection is its whole size,
    /// whatever window is cut from it, and only when it was asked for.
    #[test]
    fn the_count_of_a_merged_collection_is_its_whole_size() {
        let merged = vec![entity("urn:a"), entity("urn:b"), entity("urn:c")];
        let (page, total) = merged_page(10, PageStart::Offset(3), ResultCount::Requested).cut(merged, Some(1));
        assert!(page.is_empty());
        pretty_assertions::assert_eq!(total, Some(3));
        let (_, total) = merged_page(10, PageStart::Offset(0), ResultCount::NotRequested).cut(vec![entity("urn:a")], Some(1));
        pretty_assertions::assert_eq!(total, None);
    }

    /// A page the store cut is answered as the store answered it.
    #[test]
    fn a_locally_cut_page_is_left_as_the_store_ordered_it() {
        let page = QueryPage {
            cut_at: PageCut::Local,
            ..merged_page(10, PageStart::Offset(0), ResultCount::Requested)
        };
        let (entities, total) = page.cut(vec![entity("urn:b"), entity("urn:a")], Some(7));
        pretty_assertions::assert_eq!(ids(&entities), vec!["urn:b", "urn:a"]);
        pretty_assertions::assert_eq!(total, Some(7));
    }

    /// Clause 5.5.9.1: a fresh query in the default order with no offset walks
    /// by cursor whatever its scope, since a distributed query is paged in the
    /// same identifier order as a local one.
    #[test]
    fn a_fresh_default_order_query_walks_by_cursor_whatever_its_scope() {
        let mut query = parameters("type=Vehicle&local=false");
        let page = QueryPage::of(&mut query, None, PagePointers::OverRequestUri, None);
        pretty_assertions::assert_eq!(page.window.start, PageStart::Cursor(None));
    }

    /// Clause 5.5.9.2: an explicit offset keeps the transparent pointers.
    #[test]
    fn an_explicit_offset_keeps_the_offset_pointers() {
        let mut query = parameters("type=Vehicle&offset=4");
        let page = QueryPage::of(&mut query, None, PagePointers::OverRequestUri, None);
        pretty_assertions::assert_eq!(page.window.start, PageStart::Offset(4));
    }

    /// Clause 4.23: a client-requested Entity ordering keeps the offset
    /// pointers, since a cursor walks the identifier order alone.
    #[test]
    fn an_explicit_ordering_keeps_the_offset_pointers() {
        let mut query = parameters("type=Vehicle&orderBy=speed");
        let page = QueryPage::of(&mut query, None, PagePointers::OverRequestUri, None);
        pretty_assertions::assert_eq!(page.window.start, PageStart::Offset(0));
    }

    /// A continuation keeps walking by the cursor it arrived with.
    #[test]
    fn a_continuation_keeps_its_incoming_cursor() {
        let mut query = parameters("type=Vehicle");
        let incoming = PageCursor::After(uri("urn:b"));
        let page = QueryPage::of(&mut query, None, PagePointers::OverRequestUri, Some(incoming.clone()));
        pretty_assertions::assert_eq!(page.window.start, PageStart::Cursor(Some(incoming)));
    }
}
