//! The matching entities an `EntityMap` creation fixes (clauses 5.14.4.4 and
//! 5.14.5.4): every one of them.
//!
//! Clauses 5.14.4.3 and 5.14.5.3 list the clause 5.5.9 limit as an input the
//! creation ignores, and name no offset: the map fixes the whole candidate set
//! a later query pages through, so neither `limit` nor `offset` narrows it. A
//! creation hands out no page of its own to walk, so it can carry no cursor:
//! the response is the map, never a collection with neighbouring pages. The
//! request still goes through the broker's one pagination policy like every
//! other collection, which refuses a cursor here before anything is forwarded
//! to a context source.

use crate::error::EntityMapResultExt;
use andromeda_http_extraction::pagination_policy::RequestPagination;
use andromeda_http_response::problem_response::ProblemResponse;
use andromeda_ngsi_ld_query::{count_policy::CountPolicy, ordering::parameters::OrderingParameters, page_window::PageWindow};
use andromeda_ngsi_ld_request::pagination::{
    page_pointers::PagePointers,
    page_request::{CollectionOrder, PageRequest},
    page_size::{PageSizeBounds, PageSizeCap},
    parameters::PageParameters,
};
use axum::http::Uri;

/// The page size the pagination policy reads a creation request with: every
/// match by default, and no cap on a `limit` the creation ignores anyway.
const ENTITY_MAP_PAGE_SIZE: PageSizeBounds = PageSizeBounds {
    default: u32::MAX,
    cap: PageSizeCap::Uncapped,
};

/// The window of matching entities a creation request fixes in its map:
/// unbounded, whatever `limit` and `offset` the request names.
///
/// # Errors
///
/// Returns the `InvalidRequest` a `cursor` is answered with on an operation
/// that hands out no page pointer (clause 6.3.20), and whatever else the
/// pagination policy refuses.
pub(crate) fn membership_window(
    pagination: &RequestPagination,
    page: &PageParameters,
    ordering: &OrderingParameters,
    uri: &Uri,
) -> Result<PageWindow, ProblemResponse> {
    pagination
        .policy
        .window(
            page,
            PageRequest {
                size: ENTITY_MAP_PAGE_SIZE,
                order: CollectionOrder::of(ordering),
                count: CountPolicy::Emit,
                pointers: PagePointers::Absent,
                context: &pagination.context,
                query: &pagination.query,
            },
        )
        .map(|_ignored_page| PageWindow::unbounded())
        .with_entity_map_instance(uri)
}

#[cfg(test)]
mod tests {
    use crate::membership_window::membership_window;
    use andromeda_http_extraction::pagination_policy::RequestPagination;
    use andromeda_http_response::link_target::PointedRequest;
    use andromeda_ngsi_ld_query::{ordering::parameters::OrderingParameters, page_window::PageWindow};
    use andromeda_ngsi_ld_request::pagination::{
        cursor_request::CursorRequest,
        page_context::PageContext,
        page_query::PageQuery,
        parameters::PageParameters,
        policy::PaginationPolicy,
    };
    use axum::{
        http::{StatusCode, Uri},
        response::IntoResponse,
    };
    use pretty_assertions::assert_eq;

    fn cursor_default() -> RequestPagination {
        RequestPagination {
            policy: PaginationPolicy::CursorDefault,
            context: PageContext::try_from("https://example.com/core.jsonld").expect("an IRI"),
            query: PageQuery::of(None),
            pointed: PointedRequest::new(None, uri()),
        }
    }

    fn uri() -> Uri {
        "/ngsi-ld/v1/entityMaps".parse().expect("valid URI")
    }

    #[test]
    fn an_unpaged_creation_fixes_every_match() {
        let window = membership_window(&cursor_default(), &PageParameters::default(), &OrderingParameters::default(), &uri()).ok();
        assert_eq!(window, Some(PageWindow::unbounded()));
    }

    /// Clauses 5.14.4.3 and 5.14.5.3: the limit is ignored, and the map fixes
    /// the whole candidate set, so an offset places nothing either.
    #[test]
    fn a_limit_and_an_offset_leave_the_map_fixing_every_match() {
        for (limit, offset) in [(Some(1), None), (Some(0), None), (Some(5), Some(10)), (None, Some(3))] {
            let page = PageParameters {
                limit,
                offset,
                cursor: None,
                context: None,
                query: None,
            };
            let window = membership_window(&cursor_default(), &page, &OrderingParameters::default(), &uri()).ok();
            assert_eq!(window, Some(PageWindow::unbounded()), "limit {limit:?}, offset {offset:?}");
        }
    }

    /// Clause 6.3.20: a creation hands out no pointer, so a cursor is a
    /// parameter it does not offer.
    #[test]
    fn a_cursor_is_refused_as_invalid_request() {
        let page = PageParameters {
            cursor: Some(CursorRequest::Start),
            ..PageParameters::default()
        };
        let refusal = membership_window(&cursor_default(), &page, &OrderingParameters::default(), &uri()).expect_err("refused");
        assert_eq!(refusal.into_response().status(), StatusCode::BAD_REQUEST);
    }
}
