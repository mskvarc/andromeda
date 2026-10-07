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
use andromeda_http_headers::media_type::ResponseMediaType;
use andromeda_http_response::pagination::{PageOutcome, append_page_links};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_query::{count_policy::CountPolicy, page_window::PageWindow};
use andromeda_ngsi_ld_request::{
    error::ParameterError,
    pagination::{
        page_pointers::PagePointers,
        page_request::{CollectionOrder, PageRequest},
        policy::PaginationPolicy,
    },
    query::{QUERY_PAGE_SIZE, entity::parameters::EntityQueryParameters},
};
use axum::{http::Uri as HttpUri, response::Response};
use std::borrow::Cow;

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
    pagination: PaginationPolicy,
    query: &EntityQueryParameters,
    count: CountPolicy,
    pointers: PagePointers,
) -> Result<PageWindow, ParameterError> {
    let request = PageRequest {
        size: QUERY_PAGE_SIZE,
        order: CollectionOrder::of(&query.ordering),
        count,
        pointers,
    };
    pagination.window(&query.page, request)
}

impl QueryPage {
    /// The page `window` selects, counted as `count` says.
    ///
    /// Clause 5.5.9: once sources contribute entities the page is cut from the
    /// merged collection rather than from the local rows, in the same `id`
    /// order the local store pages in, so a cursor serves the merged collection
    /// as well as the local one. Clause 4.23.1's "sort ordering is never
    /// applied to distributed operations" concerns the clause 4.23 ordering a
    /// client requests with `orderBy`, which pages by offset.
    pub(crate) fn of(window: PageWindow, count: CountPolicy, forwarded: Option<&ForwardedQueryResults>, pointers: PagePointers) -> QueryPage {
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
        }
    }

    /// The window the local store reads, and whether it counts.
    ///
    /// Without a merge the store reads the requested window itself. With one,
    /// it reads the first local rows up to the merge budget, uncounted: the
    /// window and the total are both taken from the merged collection.
    pub(crate) const fn local_read(&self) -> (Cow<'_, PageWindow>, CountPolicy) {
        match self.cut_at {
            PageCut::Local => (Cow::Borrowed(&self.window), self.count),
            PageCut::AfterMerge { local_rows } => (Cow::Owned(PageWindow::offset(local_rows, 0)), CountPolicy::Omit),
        }
    }

    /// Cut the page out of the collection the query answers with, and report
    /// the total alongside it when the request asked to be told.
    ///
    /// `local_total` is what the store counted, which stands whenever the store
    /// held the whole collection. A merged collection is ordered by `id` before
    /// the window is cut from it.
    pub(crate) fn cut<T>(&self, entities: Vec<T>, id: impl Fn(&T) -> &str, local_total: Option<i64>) -> (Vec<T>, Option<i64>) {
        match self.cut_at {
            PageCut::Local => (entities, local_total),
            PageCut::AfterMerge { .. } => {
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
                (self.window.cut(entities, id), total)
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
            PagePointers::OverRequestUri => append_page_links(response, instance, &self.window, PageOutcome { total, returned, boundary }, media_type),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::entity::query_page::{PageCut, QueryPage};
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use andromeda_ngsi_ld_model::entity::Entity;
    use andromeda_ngsi_ld_query::{
        count_policy::CountPolicy,
        cursor::PageCursor,
        page_window::{PageStart, PageWindow},
    };
    use andromeda_ngsi_ld_request::pagination::page_pointers::PagePointers;
    use serde_json::json;
    use std::borrow::Cow;

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
        let (page, total) = merged_page(2, PageStart::Offset(1), CountPolicy::Emit).cut(five_merged(), |entity| entity.id.as_str(), None);
        pretty_assertions::assert_eq!(ids(&page), vec!["urn:b", "urn:c"]);
        pretty_assertions::assert_eq!(total, Some(5));
    }

    /// Clause 5.5.9.1: a cursor continues a walk over the merged collection
    /// from its boundary, in the same identifier order the offset cut uses,
    /// and the count stays the size of the whole collection (clause 6.3.13).
    #[test]
    fn a_cursor_continues_a_walk_over_the_merged_collection() {
        let start = PageStart::Cursor(Some(PageCursor::After(uri("urn:b"))));
        let (page, total) = merged_page(2, start, CountPolicy::Emit).cut(five_merged(), |entity| entity.id.as_str(), None);
        pretty_assertions::assert_eq!(ids(&page), vec!["urn:c", "urn:d"]);
        pretty_assertions::assert_eq!(total, Some(5));
    }

    /// A backward cursor over the merged collection returns the page before its
    /// boundary, in ascending order.
    #[test]
    fn a_backward_cursor_pages_back_through_the_merged_collection() {
        let start = PageStart::Cursor(Some(PageCursor::Before(uri("urn:d"))));
        let (page, _) = merged_page(2, start, CountPolicy::Omit).cut(five_merged(), |entity| entity.id.as_str(), None);
        pretty_assertions::assert_eq!(ids(&page), vec!["urn:b", "urn:c"]);
    }

    /// Clause 6.3.13: the count of a merged collection is its whole size,
    /// whatever window is cut from it, and only when it was asked for.
    #[test]
    fn the_count_of_a_merged_collection_is_its_whole_size() {
        let merged = vec![entity("urn:a"), entity("urn:b"), entity("urn:c")];
        let (page, total) = merged_page(10, PageStart::Offset(3), CountPolicy::Emit).cut(merged, |entity| entity.id.as_str(), Some(1));
        assert!(page.is_empty());
        pretty_assertions::assert_eq!(total, Some(3));
        let (_, total) = merged_page(10, PageStart::Offset(0), CountPolicy::Omit).cut(vec![entity("urn:a")], |entity| entity.id.as_str(), Some(1));
        pretty_assertions::assert_eq!(total, None);
    }

    /// A page the store cut is answered as the store answered it.
    #[test]
    fn a_locally_cut_page_is_left_as_the_store_ordered_it() {
        let page = QueryPage {
            cut_at: PageCut::Local,
            ..merged_page(10, PageStart::Offset(0), CountPolicy::Emit)
        };
        let (entities, total) = page.cut(vec![entity("urn:b"), entity("urn:a")], |entity| entity.id.as_str(), Some(7));
        pretty_assertions::assert_eq!(ids(&entities), vec!["urn:b", "urn:a"]);
        pretty_assertions::assert_eq!(total, Some(7));
    }

    /// Without a merge the store reads the requested window, counted as asked.
    #[test]
    fn a_local_page_reads_the_requested_window() {
        let window = PageWindow::after(5, uri("urn:b"));
        let page = QueryPage::of(window.clone(), CountPolicy::Emit, None, PagePointers::OverRequestUri); // clone: the expectation keeps the window
        pretty_assertions::assert_eq!(page.local_read(), (Cow::Borrowed(&window), CountPolicy::Emit));
    }

    /// Clause 5.5.9: a merge reads its local rows through a window of its own,
    /// uncounted, and leaves the requested window -- cursor and all -- to be
    /// cut from the merged collection.
    #[test]
    fn merge_widening_reads_a_separate_local_window_without_touching_params() {
        let start = PageStart::Cursor(Some(PageCursor::After(uri("urn:b"))));
        let page = merged_page(2, start.clone(), CountPolicy::Emit); // clone: the expectation keeps the start
        let (local, count) = page.local_read();
        pretty_assertions::assert_eq!(local.into_owned(), PageWindow::offset(101, 0));
        pretty_assertions::assert_eq!(count, CountPolicy::Omit);
        pretty_assertions::assert_eq!(page.window, PageWindow { limit: 2, start });
    }
}
