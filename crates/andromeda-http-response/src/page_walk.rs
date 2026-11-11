//! What every pointer of a page holds the followed request to (clause
//! 5.5.9.1).
//!
//! Clause 5.5.9.1 keeps one `@context` throughout a pagination, and a cursor
//! names only a position, so a followed pointer has to name the walk it
//! belongs to. A query-string walk names it by the `@context` the page was
//! read under and, beside a cursor, by the digest of its query. A posted
//! query's walk names it by the key of the query the broker holds, which
//! stands for both.

use andromeda_ngsi_ld_request::pagination::{page_context::PageContext, page_query::PageQuery};

/// The walk a page's pointers belong to, as each pointer names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageWalk<'walk> {
    /// The pointers rebuild the request's query string: each carries the
    /// `@context` the page was read under, and a cursor pointer also the
    /// digest of the query.
    QueryString {
        /// The `@context` the page was read under.
        context: &'walk PageContext,
        /// The digest of the query the page answered.
        query: &'walk PageQuery,
    },
    /// The pointers name a posted query the broker holds, by the key that
    /// stands for its query and the `@context` it was read under.
    PostedQuery(&'walk PageQuery),
}
