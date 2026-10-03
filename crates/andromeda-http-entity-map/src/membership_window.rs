//! The page of matching entities an `EntityMap` creation fixes (clauses
//! 5.14.4.4 and 5.14.5.4).
//!
//! A creation request is a query, and its `limit`/`offset` narrow the entities
//! the map fixes as they narrow a query's answer. It hands out no page of its
//! own to walk, so it can carry no cursor: the response is the map, never a
//! collection with neighbouring pages. The decision goes through the broker's
//! one pagination policy like every other collection, which refuses a cursor
//! here before anything is forwarded to a context source.

use crate::error::EntityMapResultExt;
use andromeda_http_response::problem_response::ProblemResponse;
use andromeda_ngsi_ld_query::{count_policy::CountPolicy, ordering::parameters::OrderingParameters, page_window::PageWindow};
use andromeda_ngsi_ld_request::pagination::{
    page_pointers::PagePointers,
    page_request::{CollectionOrder, PageRequest},
    page_size::{PageSizeBounds, PageSizeCap},
    parameters::PageParameters,
    policy::PaginationPolicy,
};
use axum::http::Uri;

/// The page size of a map's membership: every match unless the request names
/// a `limit`, and whatever it names otherwise, since the map is meant to fix
/// the whole result set a later walk pages through.
const ENTITY_MAP_PAGE_SIZE: PageSizeBounds = PageSizeBounds {
    default: u32::MAX,
    cap: PageSizeCap::Uncapped,
};

/// The window of matching entities a creation request fixes in its map.
///
/// # Errors
///
/// Returns the `InvalidRequest` a `cursor` is answered with on an operation
/// that hands out no page pointer (clause 6.3.20), and whatever else the
/// pagination policy refuses.
pub(crate) fn membership_window(
    pagination: PaginationPolicy,
    page: &PageParameters,
    ordering: &OrderingParameters,
    uri: &Uri,
) -> Result<PageWindow, ProblemResponse> {
    pagination
        .window(
            page,
            PageRequest {
                size: ENTITY_MAP_PAGE_SIZE,
                order: CollectionOrder::of(ordering),
                count: CountPolicy::Emit,
                pointers: PagePointers::NotDereferenceable,
            },
        )
        .with_entity_map_instance(uri)
}

#[cfg(test)]
mod tests {
    use crate::membership_window::membership_window;
    use andromeda_ngsi_ld_query::{ordering::parameters::OrderingParameters, page_window::PageWindow};
    use andromeda_ngsi_ld_request::pagination::{cursor_request::CursorRequest, parameters::PageParameters, policy::PaginationPolicy};
    use axum::{
        http::{StatusCode, Uri},
        response::IntoResponse,
    };
    use pretty_assertions::assert_eq;

    fn uri() -> Uri {
        "/ngsi-ld/v1/entityMaps".parse().expect("valid URI")
    }

    /// A creation naming no page fixes every match.
    #[test]
    fn an_unpaged_creation_fixes_every_match() {
        let window = membership_window(
            PaginationPolicy::CursorDefault,
            &PageParameters::default(),
            &OrderingParameters::default(),
            &uri(),
        )
        .ok();
        assert_eq!(window, Some(PageWindow::offset(u32::MAX, 0)));
    }

    /// A creation's `limit` and `offset` narrow the map like a query's answer.
    #[test]
    fn limit_and_offset_narrow_the_map() {
        let page = PageParameters {
            limit: Some(5),
            offset: Some(10),
            cursor: None,
        };
        let window = membership_window(PaginationPolicy::CursorDefault, &page, &OrderingParameters::default(), &uri()).ok();
        assert_eq!(window, Some(PageWindow::offset(5, 10)));
    }

    /// Clause 6.3.20: a creation hands out no pointer, so a cursor is a
    /// parameter it does not offer.
    #[test]
    fn a_cursor_is_refused_as_invalid_request() {
        let page = PageParameters {
            cursor: Some(CursorRequest::Start),
            ..PageParameters::default()
        };
        let refusal = membership_window(PaginationPolicy::CursorDefault, &page, &OrderingParameters::default(), &uri()).expect_err("refused");
        assert_eq!(refusal.into_response().status(), StatusCode::BAD_REQUEST);
    }
}
