//! The page an entity query answers with (clause 5.5.9).
//!
//! Where the page is cut depends on who holds the entities. With nothing
//! forwarded, the store already returned the window the request asked for.
//! Once context sources contribute entities of their own, the local rows are
//! no longer the collection being paged: the local query then runs unpaged and
//! the window is cut from the merged collection instead.

use crate::entity::query_forwarding::ForwardedQueryResults;
use andromeda_http_headers::media_type::ResponseMediaType;
use andromeda_http_response::pagination::{PageExtent, PaginationStyle, append_pagination_links};
use andromeda_ngsi_ld_model::entity::Entity;
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
    /// The clause 6.3.10 page size.
    limit: u32,
    /// The clause 6.3.10 page start.
    offset: u32,
    /// Whether the total is reported.
    count: ResultCount,
    /// Which collection the window is cut from.
    cut_at: PageCut,
}

impl QueryPage {
    /// The page `query` asked for.
    ///
    /// Clause 5.5.9: once sources contribute entities the page is cut from the
    /// merged collection rather than from the local rows, so the local query is
    /// widened to the whole result here and the requested window is applied by
    /// [`QueryPage::cut`] once the merge is done.
    pub(crate) fn of(query: &mut EntityQueryParameters, forwarded: Option<&ForwardedQueryResults>) -> QueryPage {
        let page = QueryPage {
            limit: query.effective_limit(),
            offset: query.effective_offset(),
            count: match query.count {
                Some(true) => ResultCount::Requested,
                Some(false) | None => ResultCount::NotRequested,
            },
            cut_at: if forwarded.is_some_and(ForwardedQueryResults::holds_remote_entities) {
                PageCut::AfterMerge
            } else {
                PageCut::Local
            },
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
    /// A query that asked for a count knows its total and drives `next` from
    /// it; one that did not carries no total and signals a following page by
    /// the page coming back full.
    pub(crate) fn append_links(&self, response: &mut Response, instance: &HttpUri, media_type: ResponseMediaType, total: Option<i64>, returned: usize) {
        let extent = match total {
            Some(count) => PageExtent::Total(count),
            None => PageExtent::PageFullness {
                returned: i64::try_from(returned).unwrap_or(i64::MAX),
            },
        };
        append_pagination_links(
            response,
            instance,
            i64::from(self.limit),
            i64::from(self.offset),
            extent,
            PaginationStyle::OffsetPrevNext,
            media_type,
        );
    }
}
