//! Which way one ordering term sorts (clause 4.23.1).

use crate::ordering::term::OrderDirection;

/// The two directions a sort key can run in.
///
/// Clause 4.23.1 offers four sort orders, but two of them differ only in what
/// is measured: `dist-asc` and `dist-desc` sort by distance rather than by
/// value, ascending and descending exactly as `asc` and `desc` do. Every sort
/// key a backend builds is built from this reduction, so no key has to
/// re-derive which of the four term directions it is under, and an ordering
/// that admits no distance term — the temporal one of clause 5.7.4.3 — carries
/// this rather than the four-way direction it could not satisfy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SortDirection {
    /// Smallest first.
    Ascending,
    /// Largest first.
    Descending,
}

impl From<OrderDirection> for SortDirection {
    fn from(direction: OrderDirection) -> SortDirection {
        match direction {
            OrderDirection::Asc | OrderDirection::DistAsc => SortDirection::Ascending,
            OrderDirection::Desc | OrderDirection::DistDesc => SortDirection::Descending,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::ordering::{sort_direction::SortDirection, term::OrderDirection};
    use pretty_assertions::assert_eq;

    /// Clause 4.23.1's four sort orders differ in what they measure, not in how
    /// many directions there are.
    #[test]
    fn a_distance_direction_sorts_the_same_way_its_value_counterpart_does() {
        assert_eq!(SortDirection::from(OrderDirection::Asc), SortDirection::Ascending);
        assert_eq!(SortDirection::from(OrderDirection::DistAsc), SortDirection::Ascending);
        assert_eq!(SortDirection::from(OrderDirection::Desc), SortDirection::Descending);
        assert_eq!(SortDirection::from(OrderDirection::DistDesc), SortDirection::Descending);
    }
}
