//! The compiled, context-expanded form of an entity query.

use crate::{entity::selection::EntitySelection, ordering::compiled::ResultOrdering, pagination::Pagination, projection::compiled::Projection};
use typed_builder::TypedBuilder;

/// A fully compiled entity query, ready for SQL generation.
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
    /// Which members of a matching entity reach the response (clause 6.3.15).
    #[builder(default)]
    pub projection: Projection,
    /// How matching entities are ordered (clause 4.23).
    #[builder(default)]
    pub ordering: ResultOrdering,
    /// Result window: limit, offset, and the total-count policy.
    #[builder(default = Pagination::DEFAULT)]
    pub pagination: Pagination,
}
