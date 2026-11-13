//! What one collection contributes to the decision of which page it answers
//! with.

use crate::pagination::{page_context::PageContext, page_pointers::PagePointers, page_size::PageSizeBounds, query_digest::QueryDigest};
use andromeda_ngsi_ld_query::{count_policy::CountPolicy, ordering::parameters::OrderingParameters};

/// The facts about one paged request that the pagination policy cannot learn
/// from its `limit`/`offset`/`cursor` alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageRequest<'context> {
    /// The collection's default and maximum page size.
    pub size: PageSizeBounds,
    /// The order the collection is read in.
    pub order: CollectionOrder,
    /// Whether the request asked for the clause 6.3.13 total.
    pub count: CountPolicy,
    /// Whether the response can carry pointers to its neighbouring pages.
    pub pointers: PagePointers,
    /// The `@context` the request itself is read under, which a followed
    /// pointer's must equal (clause 5.5.9.1).
    pub context: &'context PageContext,
    /// The digest of the request's own query, which a followed cursor
    /// pointer's must equal (clause 5.5.9.1).
    pub query: &'context QueryDigest,
}

/// The order a paged collection is read in.
///
/// A cursor is a position in identifier order (clause 5.5.9.1), so only a
/// collection read in that order can be walked by one. A clause 4.23 ordering
/// the client requested is paged by offset alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CollectionOrder {
    /// The default order, by identifier.
    Identifier,
    /// An order the request asked for with `orderBy`.
    Requested,
}

impl CollectionOrder {
    /// The order a query with these clause 4.23 ordering parameters is read in.
    #[must_use]
    pub const fn of(ordering: &OrderingParameters) -> CollectionOrder {
        if ordering.order_by.is_empty() {
            CollectionOrder::Identifier
        } else {
            CollectionOrder::Requested
        }
    }
}
