//! The compiled `orderBy` clause of a query (clause 4.23).

use crate::ordering::{geometry::OrderingGeometry, term::CompiledOrderTerm};
use typed_builder::TypedBuilder;

/// How a query orders the entities it matched.
///
/// The reference geometry is an ordering input (`orderFrom` / `orderGeometry` name the point distance is measured from), unrelated to the `georel` predicate that decided which entities matched.
#[derive(Debug, Clone, Default, TypedBuilder)]
#[builder(field_defaults(default = Default::default()))]
pub struct ResultOrdering {
    /// Ordering terms, applied left to right. Empty means the broker's default order.
    pub terms: Vec<CompiledOrderTerm>,
    /// Reference geometry for distance-based ordering (clause 5.2.43).
    pub geometry: Option<OrderingGeometry>,
}
