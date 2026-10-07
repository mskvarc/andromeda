//! The one decision of which page a request is answered with.

use crate::{
    error::ParameterError,
    pagination::{
        cursor_availability::CursorAvailability,
        cursor_request::CursorRequest,
        page_pointers::PagePointers,
        page_request::{CollectionOrder, PageRequest},
        paging_mode::PagingMode,
        parameters::PageParameters,
    },
};
use andromeda_ngsi_ld_query::{
    count_policy::CountPolicy,
    page_window::{PageStart, PageWindow},
};
use thiserror::Error;

/// How the broker pages its collections, resolved once at startup from the
/// operator's `[pagination] default_mode` and whether cursor pagination is
/// available.
///
/// Clause 4.12 leaves the mechanism to the broker and clause 5.5.9 offers
/// both: offset pagination (clause 5.5.9.2) is always available, opaque
/// cursors (clause 5.5.9.1) only while the `cursor_pagination` extension is on.
/// A default of cursors without cursors is refused at resolution, so it cannot
/// be represented here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaginationPolicy {
    /// A fresh request walks by cursor; `offset` and `cursor=` both remain
    /// available.
    CursorDefault,
    /// A fresh request pages by offset; an empty `cursor=` opts into a walk.
    OffsetDefault,
    /// Only offsets: a `cursor` is refused.
    OffsetOnly,
}

/// A pagination configuration no policy satisfies.
#[derive(Debug, Clone, Copy, Error, PartialEq, Eq)]
pub enum PaginationPolicyError {
    /// The operator made cursors the default and disabled them.
    #[error(
        "pagination.default_mode = \"cursor\" requires extensions.cursor_pagination = \"enabled\"; enable cursor pagination or set pagination.default_mode = \"offset\""
    )]
    CursorDefaultWithoutCursorPagination,
}

impl PaginationPolicy {
    /// Resolve the policy from the configured default mode and whether cursors
    /// are available.
    ///
    /// The conformance override forcing cursors off overrides whatever default
    /// the operator chose, so the broker falls back to offsets; an operator
    /// disabling cursors while keeping them the default has written a
    /// contradiction instead.
    ///
    /// # Errors
    ///
    /// Returns [`PaginationPolicyError::CursorDefaultWithoutCursorPagination`]
    /// when the operator disabled cursor pagination and left it the default.
    pub const fn resolve(default_mode: PagingMode, cursor: CursorAvailability) -> Result<PaginationPolicy, PaginationPolicyError> {
        match (default_mode, cursor) {
            (PagingMode::Cursor, CursorAvailability::Enabled) => Ok(PaginationPolicy::CursorDefault),
            (PagingMode::Offset, CursorAvailability::Enabled) => Ok(PaginationPolicy::OffsetDefault),
            (PagingMode::Offset, CursorAvailability::DisabledByOperator | CursorAvailability::ForcedOffByConformance)
            | (PagingMode::Cursor, CursorAvailability::ForcedOffByConformance) => Ok(PaginationPolicy::OffsetOnly),
            (PagingMode::Cursor, CursorAvailability::DisabledByOperator) => Err(PaginationPolicyError::CursorDefaultWithoutCursorPagination),
        }
    }

    /// The window `page` asks for of the collection `request` describes.
    ///
    /// Every pagination rule is applied here, in this order: a followed pointer
    /// under another `@context`, a cursor where none can be followed, a cursor
    /// beside an offset, a cursor over a requested ordering, then `limit=0`
    /// without a count; then the limit is bounded and the start decided.
    ///
    /// # Errors
    ///
    /// - [`ParameterError::PageContextChanged`] for a followed pointer whose
    ///   `@context` is not the request's (clause 5.5.9.1), by offset or by
    ///   cursor alike.
    /// - [`ParameterError::CursorUnavailable`] for a `cursor` under
    ///   [`PaginationPolicy::OffsetOnly`] or on a binding whose pointers cannot
    ///   be dereferenced: a parameter the operation does not offer
    ///   (`InvalidRequest`, clause 6.3.20).
    /// - [`ParameterError::CursorWithOffset`] for `cursor` and `offset`
    ///   together (clause 5.5.9).
    /// - [`ParameterError::CursorWithOrdering`] for a `cursor` over a clause
    ///   4.23 ordering, which a cursor's identifier position cannot walk.
    /// - [`ParameterError::LimitZeroWithoutCount`] for `limit=0` without a
    ///   count (Table 6.3.10-1).
    pub fn window(self, page: &PageParameters, request: PageRequest<'_>) -> Result<PageWindow, ParameterError> {
        if page.context.as_ref().is_some_and(|walked| walked != request.context) {
            return Err(ParameterError::PageContextChanged);
        }
        if page.cursor.is_some() {
            refuse_unwalkable_cursor(self, page, request)?;
        }
        if page.limit == Some(0) && request.count == CountPolicy::Omit {
            return Err(ParameterError::LimitZeroWithoutCount);
        }
        let start = match (&page.cursor, page.offset) {
            // clone: the window owns the position the followed pointer carried.
            (Some(CursorRequest::Continue(cursor)), _) => PageStart::Cursor(Some(cursor.clone())),
            (Some(CursorRequest::Start), _) => PageStart::Cursor(None),
            (None, Some(offset)) => PageStart::Offset(offset),
            (None, None) => self.fresh_start(request),
        };
        Ok(PageWindow {
            limit: request.size.limit(page.limit),
            start,
        })
    }

    /// Where a request naming neither `offset` nor `cursor` starts.
    ///
    /// A collection in a requested order, or one whose pointers cannot be
    /// followed, can only be paged by offset; any other follows the policy's
    /// default.
    const fn fresh_start(self, request: PageRequest<'_>) -> PageStart {
        match (request.pointers, request.order) {
            (PagePointers::NotDereferenceable, CollectionOrder::Identifier | CollectionOrder::Requested)
            | (PagePointers::OverRequestUri, CollectionOrder::Requested) => PageStart::Offset(0),
            (PagePointers::OverRequestUri, CollectionOrder::Identifier) => match self {
                PaginationPolicy::CursorDefault => PageStart::Cursor(None),
                PaginationPolicy::OffsetDefault | PaginationPolicy::OffsetOnly => PageStart::Offset(0),
            },
        }
    }
}

/// Refuse a `cursor` the collection cannot be walked by.
const fn refuse_unwalkable_cursor(policy: PaginationPolicy, page: &PageParameters, request: PageRequest<'_>) -> Result<(), ParameterError> {
    match (policy, request.pointers) {
        (PaginationPolicy::OffsetOnly, PagePointers::OverRequestUri | PagePointers::NotDereferenceable)
        | (PaginationPolicy::CursorDefault | PaginationPolicy::OffsetDefault, PagePointers::NotDereferenceable) => {
            return Err(ParameterError::CursorUnavailable);
        }
        (PaginationPolicy::CursorDefault | PaginationPolicy::OffsetDefault, PagePointers::OverRequestUri) => {}
    }
    if page.offset.is_some() {
        return Err(ParameterError::CursorWithOffset);
    }
    match request.order {
        CollectionOrder::Identifier => Ok(()),
        CollectionOrder::Requested => Err(ParameterError::CursorWithOrdering),
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        error::ParameterError,
        pagination::{
            cursor_availability::CursorAvailability,
            cursor_request::CursorRequest,
            page_context::PageContext,
            page_pointers::PagePointers,
            page_request::{CollectionOrder, PageRequest},
            page_size::{PageSizeBounds, PageSizeCap},
            paging_mode::PagingMode,
            parameters::PageParameters,
            policy::{PaginationPolicy, PaginationPolicyError},
        },
    };
    use andromeda_ngsi_ld_query::{
        count_policy::CountPolicy,
        cursor::PageCursor,
        page_window::{PageStart, PageWindow},
    };
    use pretty_assertions::assert_eq;
    use std::sync::LazyLock;

    const CAPPED: PageSizeBounds = PageSizeBounds {
        default: 20,
        cap: PageSizeCap::Capped(1000),
    };

    const UNCAPPED: PageSizeBounds = PageSizeBounds {
        default: 100,
        cap: PageSizeCap::Uncapped,
    };

    /// The context a request naming none pages under.
    static CORE: LazyLock<PageContext> = LazyLock::new(|| PageContext::try_from("https://example.com/core.jsonld").expect("an IRI"));

    /// A query-string request under the core context over an
    /// identifier-ordered, uncounted collection.
    fn identifier() -> PageRequest<'static> {
        PageRequest {
            size: CAPPED,
            order: CollectionOrder::Identifier,
            count: CountPolicy::Omit,
            pointers: PagePointers::OverRequestUri,
            context: &CORE,
        }
    }

    fn user_context() -> PageContext {
        PageContext::try_from("https://example.com/a.jsonld").expect("an IRI")
    }

    fn after(id: &str) -> PageCursor {
        PageCursor::After(id.parse().expect("valid URI"))
    }

    fn page(limit: Option<u32>, offset: Option<u32>, cursor: Option<CursorRequest>) -> PageParameters {
        PageParameters {
            limit,
            offset,
            cursor,
            context: None,
        }
    }

    fn window(policy: PaginationPolicy, page: &PageParameters, request: PageRequest<'_>) -> Result<PageWindow, ParameterError> {
        policy.window(page, request)
    }

    #[test]
    fn cursor_default_with_cursor_enabled_is_cursor_default() {
        assert_eq!(
            PaginationPolicy::resolve(PagingMode::Cursor, CursorAvailability::Enabled),
            Ok(PaginationPolicy::CursorDefault)
        );
    }

    #[test]
    fn offset_default_with_cursor_enabled_is_offset_default() {
        assert_eq!(
            PaginationPolicy::resolve(PagingMode::Offset, CursorAvailability::Enabled),
            Ok(PaginationPolicy::OffsetDefault)
        );
    }

    #[test]
    fn offset_default_without_cursor_is_offset_only() {
        assert_eq!(
            PaginationPolicy::resolve(PagingMode::Offset, CursorAvailability::DisabledByOperator),
            Ok(PaginationPolicy::OffsetOnly)
        );
        assert_eq!(
            PaginationPolicy::resolve(PagingMode::Offset, CursorAvailability::ForcedOffByConformance),
            Ok(PaginationPolicy::OffsetOnly)
        );
    }

    #[test]
    fn conformance_forcing_cursor_off_makes_a_cursor_default_offset_only() {
        assert_eq!(
            PaginationPolicy::resolve(PagingMode::Cursor, CursorAvailability::ForcedOffByConformance),
            Ok(PaginationPolicy::OffsetOnly)
        );
    }

    #[test]
    fn operator_disabling_cursor_with_a_cursor_default_is_refused() {
        assert_eq!(
            PaginationPolicy::resolve(PagingMode::Cursor, CursorAvailability::DisabledByOperator),
            Err(PaginationPolicyError::CursorDefaultWithoutCursorPagination)
        );
    }

    #[test]
    fn a_fresh_query_under_cursor_default_walks_by_cursor() {
        assert_eq!(
            window(PaginationPolicy::CursorDefault, &page(None, None, None), identifier()).ok(),
            Some(PageWindow {
                limit: 20,
                start: PageStart::Cursor(None),
            })
        );
    }

    #[test]
    fn a_fresh_query_under_offset_default_pages_by_offset() {
        assert_eq!(
            window(PaginationPolicy::OffsetDefault, &page(None, None, None), identifier()).ok(),
            Some(PageWindow::offset(20, 0))
        );
    }

    #[test]
    fn a_fresh_query_under_offset_only_pages_by_offset() {
        assert_eq!(
            window(PaginationPolicy::OffsetOnly, &page(None, None, None), identifier()).ok(),
            Some(PageWindow::offset(20, 0))
        );
    }

    #[test]
    fn an_empty_cursor_opts_into_a_walk_under_offset_default() {
        let start = window(PaginationPolicy::OffsetDefault, &page(None, None, Some(CursorRequest::Start)), identifier())
            .ok()
            .map(|window| window.start);
        assert_eq!(start, Some(PageStart::Cursor(None)));
    }

    #[test]
    fn a_continuation_keeps_its_cursor() {
        let start = window(
            PaginationPolicy::CursorDefault,
            &page(None, None, Some(CursorRequest::Continue(after("urn:e:1")))),
            identifier(),
        )
        .ok()
        .map(|window| window.start);
        assert_eq!(start, Some(PageStart::Cursor(Some(after("urn:e:1")))));
    }

    #[test]
    fn an_explicit_offset_pages_by_offset() {
        let start = window(PaginationPolicy::CursorDefault, &page(None, Some(40), None), identifier())
            .ok()
            .map(|window| window.start);
        assert_eq!(start, Some(PageStart::Offset(40)));
    }

    #[test]
    fn a_requested_ordering_pages_by_offset() {
        let request = PageRequest {
            order: CollectionOrder::Requested,
            ..identifier()
        };
        let start = window(PaginationPolicy::CursorDefault, &page(None, None, None), request)
            .ok()
            .map(|window| window.start);
        assert_eq!(start, Some(PageStart::Offset(0)));
    }

    #[test]
    fn a_body_binding_pages_by_offset() {
        let request = PageRequest {
            pointers: PagePointers::NotDereferenceable,
            ..identifier()
        };
        let start = window(PaginationPolicy::CursorDefault, &page(None, None, None), request)
            .ok()
            .map(|window| window.start);
        assert_eq!(start, Some(PageStart::Offset(0)));
    }

    #[test]
    fn a_cursor_under_offset_only_is_unavailable() {
        let error = window(PaginationPolicy::OffsetOnly, &page(None, None, Some(CursorRequest::Start)), identifier()).expect_err("refused");
        assert!(matches!(error, ParameterError::CursorUnavailable), "{error}");
    }

    #[test]
    fn a_cursor_on_a_body_binding_is_unavailable() {
        let request = PageRequest {
            pointers: PagePointers::NotDereferenceable,
            ..identifier()
        };
        let error = window(
            PaginationPolicy::CursorDefault,
            &page(None, None, Some(CursorRequest::Continue(after("urn:e:1")))),
            request,
        )
        .expect_err("refused");
        assert!(matches!(error, ParameterError::CursorUnavailable), "{error}");
    }

    #[test]
    fn cursor_with_offset_is_refused() {
        let error = window(
            PaginationPolicy::CursorDefault,
            &page(None, Some(1), Some(CursorRequest::Continue(after("urn:e:1")))),
            identifier(),
        )
        .expect_err("refused");
        assert!(matches!(error, ParameterError::CursorWithOffset), "{error}");
    }

    #[test]
    fn cursor_with_requested_ordering_is_refused() {
        let request = PageRequest {
            order: CollectionOrder::Requested,
            ..identifier()
        };
        let error = window(
            PaginationPolicy::CursorDefault,
            &page(None, None, Some(CursorRequest::Continue(after("urn:e:1")))),
            request,
        )
        .expect_err("refused");
        assert!(matches!(error, ParameterError::CursorWithOrdering), "{error}");
    }

    /// Table 6.3.10-1: `limit=0` is only valid alongside a count.
    #[test]
    fn limit_zero_without_count_is_refused() {
        let error = window(PaginationPolicy::CursorDefault, &page(Some(0), None, None), identifier()).expect_err("refused");
        assert!(matches!(error, ParameterError::LimitZeroWithoutCount), "{error}");
    }

    #[test]
    fn limit_zero_with_count_is_admitted() {
        let request = PageRequest {
            count: CountPolicy::Emit,
            ..identifier()
        };
        let limit = window(PaginationPolicy::CursorDefault, &page(Some(0), None, None), request)
            .ok()
            .map(|window| window.limit);
        assert_eq!(limit, Some(0));
    }

    #[test]
    fn the_cap_bounds_the_limit() {
        let limit = window(PaginationPolicy::CursorDefault, &page(Some(5000), None, None), identifier())
            .ok()
            .map(|window| window.limit);
        assert_eq!(limit, Some(1000));
    }

    #[test]
    fn an_uncapped_collection_keeps_a_large_limit() {
        let request = PageRequest {
            size: UNCAPPED,
            ..identifier()
        };
        let limit = window(PaginationPolicy::CursorDefault, &page(Some(5000), None, None), request)
            .ok()
            .map(|window| window.limit);
        assert_eq!(limit, Some(5000));
    }

    #[test]
    fn the_default_applies_when_no_limit_is_asked() {
        let request = PageRequest {
            size: UNCAPPED,
            ..identifier()
        };
        let limit = window(PaginationPolicy::CursorDefault, &page(None, None, None), request)
            .ok()
            .map(|window| window.limit);
        assert_eq!(limit, Some(100));
    }

    /// Clause 5.5.9.1: an offset pointer followed under another `@context` is
    /// `BadRequestData`.
    #[test]
    fn an_offset_pointer_followed_under_another_context_is_refused() {
        let followed = PageParameters {
            context: Some(user_context()),
            ..page(Some(1), Some(1), None)
        };
        let error = window(PaginationPolicy::OffsetDefault, &followed, identifier()).expect_err("refused");
        assert!(matches!(error, ParameterError::PageContextChanged), "{error}");
    }

    /// Clause 5.5.9.1: a cursor pointer followed under another `@context` is
    /// refused by the same rule.
    #[test]
    fn a_cursor_pointer_followed_under_another_context_is_refused() {
        let followed = PageParameters {
            context: Some(user_context()),
            ..page(Some(1), None, Some(CursorRequest::Continue(after("urn:e:1"))))
        };
        let error = window(PaginationPolicy::CursorDefault, &followed, identifier()).expect_err("refused");
        assert!(matches!(error, ParameterError::PageContextChanged), "{error}");
    }

    #[test]
    fn a_pointer_followed_under_its_own_context_is_admitted() {
        let context = user_context();
        let followed = PageParameters {
            context: Some(user_context()),
            ..page(Some(1), Some(1), None)
        };
        let request = PageRequest {
            context: &context,
            ..identifier()
        };
        assert_eq!(window(PaginationPolicy::OffsetDefault, &followed, request).ok(), Some(PageWindow::offset(1, 1)));
    }
}
