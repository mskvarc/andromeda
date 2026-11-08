//! The page an entity query answers with (clause 5.5.9).
//!
//! Which page that is was decided by the pagination policy before the query
//! ran. Where it is cut depends on who holds the entities. With nothing
//! forwarded, the store already returned the window the request asked for.
//! Once context sources contribute entities of their own, the local rows are
//! no longer the collection being paged: the local query then reads every
//! local match up to the source budget, and the window is cut from the merged
//! collection instead. Either way the page points at its neighbours by the
//! pointer family its window carries.

use crate::entity::query_forwarding::ForwardedQueryResults;
use andromeda_http_extraction::pagination_policy::RequestPagination;
use andromeda_http_headers::media_type::ResponseMediaType;
use andromeda_http_response::{
    link_target::PointedRequest,
    pagination::{PageOutcome, append_page_links},
};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_query::{count_policy::CountPolicy, page_reach::PageReach, page_window::PageWindow};
use andromeda_ngsi_ld_request::{
    error::ParameterError,
    pagination::{
        page_context::PageContext,
        page_pointers::PagePointers,
        page_request::{CollectionOrder, PageRequest},
    },
    query::{QUERY_PAGE_SIZE, entity::parameters::EntityQueryParameters},
};
use axum::response::Response;

/// Which collection the requested window is cut from (clause 5.5.9).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PageCut {
    /// The local query returned the window itself.
    Local,
    /// Sources contributed entities, so the window is cut after the merge.
    AfterMerge {
        /// How many local rows the merge reads: one past the budget the sources
        /// were walked under (clause 4.12), which is all it takes to tell that
        /// the collection is too large.
        local_rows: u32,
    },
}

/// Whether the local store cut the page itself, and if so whether the
/// collection continues past it.
///
/// Only [`QueryPage::settle_local`] produces one, so a page the store cut is
/// never cut a second time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LocalReach {
    /// The store read the page and the element past it, and this is whether
    /// that element was there.
    Settled(PageReach),
    /// The window is cut from the merged collection.
    AfterMerge,
}

/// The window an entity query asked for, and where it is cut.
pub(crate) struct QueryPage {
    /// The page size and where the page starts, as the policy decided them.
    window: PageWindow,
    /// Whether the total is reported (Table 6.4.3.2-1 `count`).
    count: CountPolicy,
    /// Which collection the window is cut from.
    cut_at: PageCut,
    /// Whether the binding the query arrived on can carry a pointer to the
    /// neighbouring pages at all.
    pointers: PagePointers,
    /// The `@context` the query is paged under, which every pointer carries
    /// (clause 5.5.9.1).
    context: PageContext,
    /// The request every pointer rebuilds.
    pointed: PointedRequest,
}

/// The window `query` asks for under `pagination`, on a binding whose pointers
/// `pointers` describes, counted as `count` says.
///
/// Decided before anything runs, so a pagination parameter the collection
/// cannot honour is refused before a single context source is contacted.
///
/// # Errors
///
/// Returns whatever the pagination policy refuses (clauses 5.5.9 and 6.3.10).
pub(crate) fn requested_window(
    pagination: &RequestPagination,
    query: &EntityQueryParameters,
    count: CountPolicy,
    pointers: PagePointers,
) -> Result<PageWindow, ParameterError> {
    let request = PageRequest {
        size: QUERY_PAGE_SIZE,
        order: CollectionOrder::of(&query.ordering),
        count,
        pointers,
        context: &pagination.context,
        query: &pagination.query,
    };
    pagination.policy.window(&query.page, request)
}

impl QueryPage {
    /// The page `window` selects, counted as `count` says and paged under
    /// `context`.
    ///
    /// Clause 5.5.9: once sources contribute entities the page is cut from the
    /// merged collection rather than from the local rows, in the same `id`
    /// order the local store pages in, so a cursor serves the merged collection
    /// as well as the local one. Clause 4.23.1's "sort ordering is never
    /// applied to distributed operations" concerns the clause 4.23 ordering a
    /// client requests with `orderBy`, which pages by offset.
    pub(crate) fn of(
        window: PageWindow,
        count: CountPolicy,
        forwarded: Option<&ForwardedQueryResults>,
        pointers: PagePointers,
        context: PageContext,
        pointed: PointedRequest,
    ) -> QueryPage {
        let cut_at = match forwarded.filter(|results| results.holds_remote_entities()) {
            Some(results) => PageCut::AfterMerge {
                local_rows: results.local_row_limit(),
            },
            None => PageCut::Local,
        };
        QueryPage {
            window,
            count,
            cut_at,
            pointers,
            context,
            pointed,
        }
    }

    /// The window the local store reads, and whether it counts.
    ///
    /// Without a merge the store reads the requested window itself, with the
    /// element past it that tells whether the collection continues. With one,
    /// it reads the first local rows up to the merge budget, uncounted: the
    /// window and the total are both taken from the merged collection.
    pub(crate) fn local_read(&self) -> (PageWindow, CountPolicy) {
        match self.cut_at {
            PageCut::Local => (self.window.with_lookahead(), self.count),
            PageCut::AfterMerge { local_rows } => (PageWindow::offset(local_rows, 0), CountPolicy::Omit),
        }
    }

    /// Settle the rows the local store read: a window the store cut loses the
    /// element past the page, whose presence says whether the collection
    /// continues; rows a merge reads are left whole.
    ///
    /// Done on what the store read, before anything is taken out of it, so an
    /// element the page leaves out still counts towards that answer.
    pub(crate) fn settle_local<T>(&self, read: Vec<T>) -> (Vec<T>, LocalReach) {
        match self.cut_at {
            PageCut::Local => {
                let (read, reach) = self.window.settle(read);
                (read, LocalReach::Settled(reach))
            }
            PageCut::AfterMerge { .. } => (read, LocalReach::AfterMerge),
        }
    }

    /// Cut the page out of the collection the query answers with, and report
    /// whether the collection continues past it and, when the request asked to
    /// be told, the total.
    ///
    /// `local_total` is what the store counted, which stands whenever the store
    /// held the whole collection, and `local` what
    /// [`QueryPage::settle_local`] made of the store's rows. A merged
    /// collection is ordered by `id` before the window is cut from it.
    pub(crate) fn cut<T>(&self, entities: Vec<T>, id: impl Fn(&T) -> &str, local_total: Option<i64>, local: LocalReach) -> (Vec<T>, PageReach, Option<i64>) {
        match local {
            LocalReach::Settled(reach) => (entities, reach, local_total),
            LocalReach::AfterMerge => {
                let total = match self.count {
                    CountPolicy::Emit => Some(i64::try_from(entities.len()).unwrap_or(i64::MAX)),
                    CountPolicy::Omit => None,
                };
                // Clause 4.3.6.7 asks for consistent pagination over the merged
                // collection, which needs one deterministic order: the default
                // `id` order the local store pages in, byte-wise as the store
                // collates identifiers. No other order can reach this point,
                // because clause 5.7.2.4 refuses `orderBy` once a source takes
                // part.
                let mut entities = entities;
                entities.sort_unstable_by(|left, right| id(left).cmp(id(right)));
                let entities = entities;
                let (entities, reach) = self.window.cut_settled(entities, id);
                (entities, reach, total)
            }
        }
    }

    /// Add the clause 6.3.10 pagination links over this page.
    ///
    /// `boundary` is the first and last identifier of the page as displayed,
    /// from which the neighbouring cursors of a cursor page are computed, and
    /// `reach` whether the collection continues past it.
    ///
    /// A binding that carried its query in the request body emits no link:
    /// clause 6.3.10 asks for a target "that could be dereferenced by an
    /// NGSI-LD Client to retrieve the next page", and the clause 5.2.23 body of
    /// clause 6.23.3.1 is not in any URI-reference that could be.
    pub(crate) fn append_links(
        &self,
        response: &mut Response,
        media_type: ResponseMediaType,
        total: Option<i64>,
        reach: PageReach,
        boundary: Option<(Identifier, Identifier)>,
    ) {
        match self.pointers {
            PagePointers::NotDereferenceable => {}
            PagePointers::OverRequestUri => append_page_links(
                response,
                &self.pointed,
                &self.context,
                &self.window,
                PageOutcome { total, reach, boundary },
                media_type,
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::entity::query_page::{LocalReach, PageCut, QueryPage};
    use andromeda_http_response::link_target::PointedRequest;
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use andromeda_ngsi_ld_model::entity::Entity;
    use andromeda_ngsi_ld_query::{
        count_policy::CountPolicy,
        cursor::PageCursor,
        page_reach::PageReach,
        page_window::{PageStart, PageWindow},
    };
    use andromeda_ngsi_ld_request::pagination::{page_context::PageContext, page_pointers::PagePointers};
    use axum::http::Uri;
    use serde_json::json;

    fn entity(id: &str) -> Entity {
        serde_json::from_value(json!({"id": id, "type": "Vehicle"})).expect("a valid entity")
    }

    fn uri(id: &str) -> Identifier {
        id.parse().expect("valid URI")
    }

    fn merged_page(limit: u32, start: PageStart, count: CountPolicy) -> QueryPage {
        QueryPage {
            window: PageWindow { limit, start },
            count,
            cut_at: PageCut::AfterMerge { local_rows: 101 },
            pointers: PagePointers::OverRequestUri,
            context: PageContext::try_from("https://example.com/core.jsonld").expect("an IRI"),
            pointed: PointedRequest::new(None, Uri::from_static("/ngsi-ld/v1/entities")),
        }
    }

    fn ids(entities: &[Entity]) -> Vec<&str> {
        entities.iter().map(|entity| entity.id.as_str()).collect()
    }

    fn five_merged() -> Vec<Entity> {
        vec![entity("urn:c"), entity("urn:e"), entity("urn:a"), entity("urn:d"), entity("urn:b")]
    }

    /// Clause 4.3.6.7: the merged collection (local entities first, then
    /// what each source answered, in arrival order) is paged in one
    /// deterministic order, the `id` order the local store pages in.
    #[test]
    fn a_merged_collection_is_cut_from_its_identifier_order() {
        let (page, _, total) =
            merged_page(2, PageStart::Offset(1), CountPolicy::Emit).cut(five_merged(), |entity| entity.id.as_str(), None, LocalReach::AfterMerge);
        pretty_assertions::assert_eq!(ids(&page), vec!["urn:b", "urn:c"]);
        pretty_assertions::assert_eq!(total, Some(5));
    }

    /// Clause 5.5.9.1: a cursor continues a walk over the merged collection
    /// from its boundary, in the same identifier order the offset cut uses,
    /// and the count stays the size of the whole collection (clause 6.3.13).
    #[test]
    fn a_cursor_continues_a_walk_over_the_merged_collection() {
        let start = PageStart::Cursor(Some(PageCursor::After(uri("urn:b"))));
        let (page, _, total) = merged_page(2, start, CountPolicy::Emit).cut(five_merged(), |entity| entity.id.as_str(), None, LocalReach::AfterMerge);
        pretty_assertions::assert_eq!(ids(&page), vec!["urn:c", "urn:d"]);
        pretty_assertions::assert_eq!(total, Some(5));
    }

    /// A backward cursor over the merged collection returns the page before its
    /// boundary, in ascending order.
    #[test]
    fn a_backward_cursor_pages_back_through_the_merged_collection() {
        let start = PageStart::Cursor(Some(PageCursor::Before(uri("urn:d"))));
        let (page, reach, _) = merged_page(2, start, CountPolicy::Omit).cut(five_merged(), |entity| entity.id.as_str(), None, LocalReach::AfterMerge);
        pretty_assertions::assert_eq!(ids(&page), vec!["urn:b", "urn:c"]);
        pretty_assertions::assert_eq!(reach, PageReach::Continues);
    }

    /// Clause 5.5.9.1: a backward page over the merged collection that holds
    /// its first entities has no previous elements, however full it is.
    #[test]
    fn a_backward_page_holding_the_first_merged_entities_is_exhausted() {
        let start = PageStart::Cursor(Some(PageCursor::Before(uri("urn:c"))));
        let (page, reach, _) = merged_page(2, start, CountPolicy::Omit).cut(five_merged(), |entity| entity.id.as_str(), None, LocalReach::AfterMerge);
        pretty_assertions::assert_eq!(ids(&page), vec!["urn:a", "urn:b"]);
        pretty_assertions::assert_eq!(reach, PageReach::Exhausted);
    }

    /// Clause 6.3.13: the count of a merged collection is its whole size,
    /// whatever window is cut from it, and only when it was asked for.
    #[test]
    fn the_count_of_a_merged_collection_is_its_whole_size() {
        let merged = vec![entity("urn:a"), entity("urn:b"), entity("urn:c")];
        let (page, _, total) =
            merged_page(10, PageStart::Offset(3), CountPolicy::Emit).cut(merged, |entity| entity.id.as_str(), Some(1), LocalReach::AfterMerge);
        assert!(page.is_empty());
        pretty_assertions::assert_eq!(total, Some(3));
        let (_, _, total) =
            merged_page(10, PageStart::Offset(0), CountPolicy::Omit).cut(vec![entity("urn:a")], |entity| entity.id.as_str(), Some(1), LocalReach::AfterMerge);
        pretty_assertions::assert_eq!(total, None);
    }

    /// A page the store cut is answered as the store answered it.
    #[test]
    fn a_locally_cut_page_is_left_as_the_store_ordered_it() {
        let page = QueryPage {
            cut_at: PageCut::Local,
            ..merged_page(10, PageStart::Offset(0), CountPolicy::Emit)
        };
        let (read, local) = page.settle_local(vec![entity("urn:b"), entity("urn:a")]);
        let (entities, _, total) = page.cut(read, |entity| entity.id.as_str(), Some(7), local);
        pretty_assertions::assert_eq!(ids(&entities), vec!["urn:b", "urn:a"]);
        pretty_assertions::assert_eq!(total, Some(7));
    }

    /// Clause 5.5.9.1: a page the store cut is settled on everything it read,
    /// so an entity later left out of the page still says the collection
    /// continues when it was the element past the page.
    #[test]
    fn a_locally_cut_page_is_settled_on_what_the_store_read() {
        let page = QueryPage {
            cut_at: PageCut::Local,
            ..merged_page(2, PageStart::Offset(0), CountPolicy::Omit)
        };
        let (read, local) = page.settle_local(vec![entity("urn:a"), entity("urn:b"), entity("urn:c")]);
        pretty_assertions::assert_eq!(ids(&read), vec!["urn:a", "urn:b"]);
        pretty_assertions::assert_eq!(local, LocalReach::Settled(PageReach::Continues));
        let (read, local) = page.settle_local(vec![entity("urn:a")]);
        let (entities, reach, _) = page.cut(read, |entity| entity.id.as_str(), None, local);
        pretty_assertions::assert_eq!(ids(&entities), vec!["urn:a"]);
        pretty_assertions::assert_eq!(reach, PageReach::Exhausted);
    }

    /// Rows a merge reads are left whole for the cut after the merge.
    #[test]
    fn a_merged_page_leaves_the_local_rows_whole() {
        let (read, local) = merged_page(1, PageStart::Offset(0), CountPolicy::Omit).settle_local(vec![entity("urn:a"), entity("urn:b")]);
        pretty_assertions::assert_eq!(ids(&read), vec!["urn:a", "urn:b"]);
        pretty_assertions::assert_eq!(local, LocalReach::AfterMerge);
    }

    /// Without a merge the store reads the requested window and the element
    /// past it, counted as asked.
    #[test]
    fn a_local_page_reads_the_requested_window_with_its_lookahead() {
        let page = QueryPage::of(
            PageWindow::after(5, uri("urn:b")),
            CountPolicy::Emit,
            None,
            PagePointers::OverRequestUri,
            PageContext::try_from("https://example.com/core.jsonld").expect("an IRI"),
            PointedRequest::new(None, Uri::from_static("/ngsi-ld/v1/entities")),
        );
        pretty_assertions::assert_eq!(page.local_read(), (PageWindow::after(6, uri("urn:b")), CountPolicy::Emit));
    }

    /// Clause 5.5.9: a merge reads its local rows through a window of its own,
    /// uncounted, and leaves the requested window, cursor and all, to be
    /// cut from the merged collection.
    #[test]
    fn merge_widening_reads_a_separate_local_window_without_touching_params() {
        let start = PageStart::Cursor(Some(PageCursor::After(uri("urn:b"))));
        let page = merged_page(2, start.clone(), CountPolicy::Emit);
        let (local, count) = page.local_read();
        pretty_assertions::assert_eq!(local, PageWindow::offset(101, 0));
        pretty_assertions::assert_eq!(count, CountPolicy::Omit);
        pretty_assertions::assert_eq!(page.window, PageWindow { limit: 2, start });
    }
}
