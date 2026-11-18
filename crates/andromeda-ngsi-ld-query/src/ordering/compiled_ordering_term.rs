//! An `orderBy` term compiled for a request: its expanded target, its direction and its collation (clause 4.23).

use crate::ordering::{expanded_ordering_target::ExpandedOrderingTarget, icu_collation::IcuCollation, ordering_direction::OrderingDirection};

/// A single term in an `orderBy` clause after context expansion.
#[derive(Debug, Clone)]
pub struct CompiledOrderingTerm {
    /// What the term sorts on.
    pub target: ExpandedOrderingTarget,
    /// The sort direction (ascending, descending, or distance-based).
    pub direction: OrderingDirection,
    /// ICU collation identifier for string sorting (S4.23).
    pub collation: Option<IcuCollation>,
}
