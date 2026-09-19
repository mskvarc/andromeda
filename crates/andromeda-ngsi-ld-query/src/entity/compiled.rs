//! The compiled, context-expanded form of an entity query.

use crate::{
    cursor::PageCursor,
    entity_selection::EntitySelection,
    ordering::compiled::ResultOrdering,
    pagination::Pagination,
    projection::compiled::Projection,
};
use typed_builder::TypedBuilder;

/// A fully compiled entity query, ready to be lowered to a store query.
///
/// Produced by expanding and validating `EntityQueryParameters` against the
/// active JSON-LD context. All identifiers are expanded IRIs.
///
/// The parts are held separately because they change for unrelated reasons:
/// [`EntitySelection`] decides which rows come back, [`Projection`] and
/// [`ResultOrdering`] decide only what is done with them, and the SQL builders
/// read one or the other rather than the whole record.
#[derive(Debug, Clone, TypedBuilder)]
pub struct CompiledEntityQuery {
    /// Which entities match.
    pub selection: EntitySelection,
    /// Which members of a matching entity reach the response (clause 4.21).
    #[builder(default = Default::default())]
    pub projection: Projection,
    /// How matching entities are ordered (clause 4.23).
    #[builder(default = Default::default())]
    pub ordering: ResultOrdering,
    /// Result window: limit, offset, and the total-count policy.
    #[builder(default = Pagination::DEFAULT)]
    pub pagination: Pagination,
    /// The keyset position a stable-pagination cursor resolved to, when the
    /// request carried one (clause 5.5.9.1).
    ///
    /// `Some` puts the store into keyset mode: it replaces the `offset` in
    /// [`Pagination`] with the direction-appropriate `WHERE`/`ORDER BY` over the
    /// entity `id`, and a backward or from-end page is reversed to ascending
    /// where the rows are read back. Held beside [`Pagination`] rather than
    /// inside it because it is a per-request position that never participates in
    /// compilation caching, unlike the limit and count that do.
    #[builder(default = None)]
    pub keyset: Option<PageCursor>,
}
