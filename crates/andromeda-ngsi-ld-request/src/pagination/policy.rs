//! The one decision of which page a request is answered with.

use crate::{
    error::ParameterError,
    pagination::{
        cursor_request::CursorRequest,
        page_pointers::PagePointers,
        page_request::{CollectionOrder, PageRequest},
        parameters::PageParameters,
    },
};
use andromeda_configuration::schema::extensions::{ExtensionsConfig, ExtensionsMode, cursor_pagination::CursorPaginationMode};
use andromeda_ngsi_ld_query::{
    count_policy::CountPolicy,
    page_window::{PageStart, PageWindow},
};

/// How the broker pages its collections, resolved once at startup from the
/// `[extensions]` table.
///
/// Clause 4.12 leaves the mechanism to the broker and clause 5.5.9 offers
/// both: offset pagination (clause 5.5.9.2) is always available, opaque
/// cursors (clause 5.5.9.1) only while the `cursor_pagination` extension is
/// on.
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

impl From<&ExtensionsConfig> for PaginationPolicy {
    /// The policy `extensions.cursor_pagination.mode` names, or offsets alone
    /// under the conformance override, which forces every extension off.
    fn from(extensions: &ExtensionsConfig) -> PaginationPolicy {
        match (extensions.mode, CursorPaginationMode::from(&extensions.cursor_pagination)) {
            (ExtensionsMode::Conformance, CursorPaginationMode::Disabled | CursorPaginationMode::Available | CursorPaginationMode::Default)
            | (ExtensionsMode::Configured, CursorPaginationMode::Disabled) => PaginationPolicy::OffsetOnly,
            (ExtensionsMode::Configured, CursorPaginationMode::Available) => PaginationPolicy::OffsetDefault,
            (ExtensionsMode::Configured, CursorPaginationMode::Default) => PaginationPolicy::CursorDefault,
        }
    }
}

impl PaginationPolicy {
    /// The window `page` asks for of the collection `request` describes.
    ///
    /// Every pagination rule is applied here, in this order: a followed pointer
    /// under another `@context`, a cursor where none can be followed, a cursor
    /// beside an offset, a cursor over a requested ordering, a cursor followed
    /// under another query, then `limit=0` without a count; then the start is
    /// decided and the limit checked against the collection's cap.
    ///
    /// # Errors
    ///
    /// - [`ParameterError::PageContextChanged`] for a followed pointer whose
    ///   `@context` is not the request's (clause 5.5.9.1), by offset or by
    ///   cursor alike.
    /// - [`ParameterError::CursorUnavailable`] for a `cursor` under
    ///   [`PaginationPolicy::OffsetOnly`] or on a response that hands out no
    ///   pointer: a parameter the operation does not offer (`InvalidRequest`,
    ///   clause 6.3.20).
    /// - [`ParameterError::CursorWithOffset`] for `cursor` and `offset`
    ///   together (clause 5.5.9).
    /// - [`ParameterError::CursorWithOrdering`] for a `cursor` over a clause
    ///   4.23 ordering, which a cursor's identifier position cannot walk.
    /// - [`ParameterError::CursorQueryChanged`] for a continued `cursor`
    ///   without the digest of the query it was issued for, or a digest that
    ///   is not the request's own: a position in one walk replayed over
    ///   another result.
    /// - [`ParameterError::LimitZeroWithoutCount`] for `limit=0` without a
    ///   count (Table 6.3.10-1).
    /// - [`ParameterError::PageSizeExceeded`] for a `limit` above the
    ///   collection's cap (clauses 5.5.6 and 5.5.9.1).
    pub fn window(self, page: &PageParameters, request: PageRequest<'_>) -> Result<PageWindow, ParameterError> {
        if page.context.as_ref().is_some_and(|walked| walked != request.context) {
            return Err(ParameterError::PageContextChanged);
        }
        if page.cursor.is_some() {
            refuse_unwalkable_cursor(self, page, request)?;
        }
        refuse_foreign_cursor(page, request)?;
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
            limit: request.size.limit(page.limit)?,
            start,
        })
    }

    /// Where a request naming neither `offset` nor `cursor` starts.
    ///
    /// A collection in a requested order, or a response that hands out no
    /// pointer, can only be paged by offset; any other follows the policy's
    /// default, whether its pointers rebuild the request URI or name a posted
    /// query.
    const fn fresh_start(self, request: PageRequest<'_>) -> PageStart {
        match (request.pointers, request.order) {
            (PagePointers::Absent, CollectionOrder::Identifier | CollectionOrder::Requested)
            | (PagePointers::OverRequestUri | PagePointers::OverPostedQuery, CollectionOrder::Requested) => PageStart::Offset(0),
            (PagePointers::OverRequestUri | PagePointers::OverPostedQuery, CollectionOrder::Identifier) => match self {
                PaginationPolicy::CursorDefault => PageStart::Cursor(None),
                PaginationPolicy::OffsetDefault | PaginationPolicy::OffsetOnly => PageStart::Offset(0),
            },
        }
    }
}

/// Refuse a `cursor` the collection cannot be walked by.
const fn refuse_unwalkable_cursor(policy: PaginationPolicy, page: &PageParameters, request: PageRequest<'_>) -> Result<(), ParameterError> {
    match (policy, request.pointers) {
        (PaginationPolicy::OffsetOnly, PagePointers::OverRequestUri | PagePointers::OverPostedQuery | PagePointers::Absent)
        | (PaginationPolicy::CursorDefault | PaginationPolicy::OffsetDefault, PagePointers::Absent) => {
            return Err(ParameterError::CursorUnavailable);
        }
        (PaginationPolicy::CursorDefault | PaginationPolicy::OffsetDefault, PagePointers::OverRequestUri | PagePointers::OverPostedQuery) => {}
    }
    if page.offset.is_some() {
        return Err(ParameterError::CursorWithOffset);
    }
    match request.order {
        CollectionOrder::Identifier => Ok(()),
        CollectionOrder::Requested => Err(ParameterError::CursorWithOrdering),
    }
}

/// Refuse a cursor walk continued over another query than the one its
/// pointer was issued for.
///
/// Every cursor pointer carries the digest of its query, so a continued cursor
/// without one was not followed unchanged. A digest is checked wherever it is
/// carried, so a `first` pointer is held to its query as well.
fn refuse_foreign_cursor(page: &PageParameters, request: PageRequest<'_>) -> Result<(), ParameterError> {
    let continued = matches!(page.cursor, Some(CursorRequest::Continue(_)));
    match &page.query {
        Some(walked) if walked != request.query => Err(ParameterError::CursorQueryChanged),
        None if continued => Err(ParameterError::CursorQueryChanged),
        Some(_) | None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        error::ParameterError,
        pagination::{
            cursor_request::CursorRequest,
            page_context::PageContext,
            page_pointers::PagePointers,
            page_query::PageQuery,
            page_request::{CollectionOrder, PageRequest},
            page_size::{PageSizeBounds, PageSizeCap, PageSizeExceeded},
            parameters::PageParameters,
            policy::PaginationPolicy,
        },
    };
    use andromeda_configuration::schema::extensions::{
        ExtensionsConfig,
        ExtensionsMode,
        cursor_pagination::{CursorPaginationConfig, CursorPaginationMode},
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

    /// The query every request of these tests is read for.
    static QUERY: LazyLock<PageQuery> = LazyLock::new(|| PageQuery::of(Some("type=Vehicle")));

    /// A query-string request under the core context over an
    /// identifier-ordered, uncounted collection.
    fn identifier() -> PageRequest<'static> {
        PageRequest {
            size: CAPPED,
            order: CollectionOrder::Identifier,
            count: CountPolicy::Omit,
            pointers: PagePointers::OverRequestUri,
            context: &CORE,
            query: &QUERY,
        }
    }

    fn user_context() -> PageContext {
        PageContext::try_from("https://example.com/a.jsonld").expect("an IRI")
    }

    fn after(id: &str) -> PageCursor {
        PageCursor::After(id.parse().expect("valid URI"))
    }

    /// The pagination parameters a request carries; a continued cursor carries
    /// the digest of the query it was issued for, as a followed pointer does.
    fn page(limit: Option<u32>, offset: Option<u32>, cursor: Option<CursorRequest>) -> PageParameters {
        let query = matches!(cursor, Some(CursorRequest::Continue(_))).then(|| QUERY.clone());
        PageParameters {
            limit,
            offset,
            cursor,
            context: None,
            query,
        }
    }

    fn window(policy: PaginationPolicy, page: &PageParameters, request: PageRequest<'_>) -> Result<PageWindow, ParameterError> {
        policy.window(page, request)
    }

    fn resolved(mode: ExtensionsMode, cursor: CursorPaginationMode) -> PaginationPolicy {
        PaginationPolicy::from(&ExtensionsConfig {
            mode,
            cursor_pagination: CursorPaginationConfig::from(cursor),
            ..ExtensionsConfig::default()
        })
    }

    #[test]
    fn each_cursor_pagination_mode_names_its_policy() {
        assert_eq!(
            resolved(ExtensionsMode::Configured, CursorPaginationMode::Default),
            PaginationPolicy::CursorDefault
        );
        assert_eq!(
            resolved(ExtensionsMode::Configured, CursorPaginationMode::Available),
            PaginationPolicy::OffsetDefault
        );
        assert_eq!(
            resolved(ExtensionsMode::Configured, CursorPaginationMode::Disabled),
            PaginationPolicy::OffsetOnly
        );
    }

    #[test]
    fn the_conformance_override_pages_by_offset_alone_whatever_the_mode() {
        for cursor in [CursorPaginationMode::Default, CursorPaginationMode::Available, CursorPaginationMode::Disabled] {
            assert_eq!(resolved(ExtensionsMode::Conformance, cursor), PaginationPolicy::OffsetOnly, "{cursor:?}");
        }
    }

    #[test]
    fn a_default_broker_pages_a_fresh_request_by_offset() {
        assert_eq!(PaginationPolicy::from(&ExtensionsConfig::default()), PaginationPolicy::OffsetDefault);
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
    fn a_response_without_pointers_pages_by_offset() {
        let request = PageRequest {
            pointers: PagePointers::Absent,
            ..identifier()
        };
        let start = window(PaginationPolicy::CursorDefault, &page(None, None, None), request)
            .ok()
            .map(|window| window.start);
        assert_eq!(start, Some(PageStart::Offset(0)));
    }

    /// A posted query's pointers are dereferenceable like a query string's,
    /// so its walk starts as the policy's default and continues by cursor.
    #[test]
    fn a_posted_query_walks_as_a_query_string_does() {
        let request = PageRequest {
            pointers: PagePointers::OverPostedQuery,
            ..identifier()
        };
        let start = window(PaginationPolicy::CursorDefault, &page(None, None, None), request)
            .ok()
            .map(|window| window.start);
        assert_eq!(start, Some(PageStart::Cursor(None)));
        let start = window(
            PaginationPolicy::OffsetDefault,
            &page(None, None, Some(CursorRequest::Continue(after("urn:e:1")))),
            request,
        )
        .ok()
        .map(|window| window.start);
        assert_eq!(start, Some(PageStart::Cursor(Some(after("urn:e:1")))));
        let error = window(PaginationPolicy::OffsetOnly, &page(None, None, Some(CursorRequest::Start)), request).expect_err("refused");
        assert!(matches!(error, ParameterError::CursorUnavailable), "{error}");
    }

    #[test]
    fn a_cursor_under_offset_only_is_unavailable() {
        let error = window(PaginationPolicy::OffsetOnly, &page(None, None, Some(CursorRequest::Start)), identifier()).expect_err("refused");
        assert!(matches!(error, ParameterError::CursorUnavailable), "{error}");
    }

    #[test]
    fn a_cursor_on_a_response_without_pointers_is_unavailable() {
        let request = PageRequest {
            pointers: PagePointers::Absent,
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
    fn a_limit_equal_to_the_cap_is_served() {
        let limit = window(PaginationPolicy::CursorDefault, &page(Some(1000), None, None), identifier())
            .ok()
            .map(|window| window.limit);
        assert_eq!(limit, Some(1000));
    }

    /// Clauses 5.5.6 and 5.5.9.1: a page smaller than the requested `limit`
    /// is not served in its place.
    #[test]
    fn a_limit_above_the_cap_is_refused_naming_the_cap() {
        let error = window(PaginationPolicy::CursorDefault, &page(Some(5000), None, None), identifier()).expect_err("refused");
        assert!(
            matches!(error, ParameterError::PageSizeExceeded(PageSizeExceeded { requested: 5000, cap: 1000 })),
            "{error}"
        );
    }

    /// The cap is checked by offset and by cursor alike.
    #[test]
    fn a_limit_above_the_cap_is_refused_on_a_followed_cursor() {
        let error = window(
            PaginationPolicy::CursorDefault,
            &page(Some(1001), None, Some(CursorRequest::Continue(after("urn:e:1")))),
            identifier(),
        )
        .expect_err("refused");
        assert!(matches!(error, ParameterError::PageSizeExceeded(_)), "{error}");
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

    #[test]
    fn a_cursor_followed_under_another_query_is_refused() {
        let followed = PageParameters {
            query: Some(PageQuery::of(Some("type=Vehicle&q=speed%3E10"))),
            ..page(None, None, Some(CursorRequest::Continue(after("urn:e:1"))))
        };
        let error = window(PaginationPolicy::CursorDefault, &followed, identifier()).expect_err("refused");
        assert!(matches!(error, ParameterError::CursorQueryChanged), "{error}");
    }

    /// A cursor pointer always carries its query's digest, so a continued
    /// cursor without one was not followed unchanged.
    #[test]
    fn a_continued_cursor_without_its_query_is_refused() {
        let followed = PageParameters {
            query: None,
            ..page(None, None, Some(CursorRequest::Continue(after("urn:e:1"))))
        };
        let error = window(PaginationPolicy::CursorDefault, &followed, identifier()).expect_err("refused");
        assert!(matches!(error, ParameterError::CursorQueryChanged), "{error}");
    }

    #[test]
    fn a_first_pointer_followed_under_another_query_is_refused() {
        let followed = PageParameters {
            query: Some(PageQuery::of(Some("type=Building"))),
            ..page(None, None, Some(CursorRequest::Start))
        };
        let error = window(PaginationPolicy::CursorDefault, &followed, identifier()).expect_err("refused");
        assert!(matches!(error, ParameterError::CursorQueryChanged), "{error}");
    }

    /// The digest leaves out `limit`, so a pointer may be followed with
    /// another page size.
    #[test]
    fn a_cursor_followed_with_another_limit_is_admitted() {
        let start = window(
            PaginationPolicy::CursorDefault,
            &page(Some(7), None, Some(CursorRequest::Continue(after("urn:e:1")))),
            identifier(),
        )
        .ok();
        assert_eq!(
            start,
            Some(PageWindow {
                limit: 7,
                start: PageStart::Cursor(Some(after("urn:e:1"))),
            })
        );
    }

    /// An empty `cursor=` written by the client starts a walk of whatever
    /// query it names.
    #[test]
    fn a_walk_started_without_a_digest_is_admitted() {
        let start = window(PaginationPolicy::CursorDefault, &page(None, None, Some(CursorRequest::Start)), identifier())
            .ok()
            .map(|window| window.start);
        assert_eq!(start, Some(PageStart::Cursor(None)));
    }
}
