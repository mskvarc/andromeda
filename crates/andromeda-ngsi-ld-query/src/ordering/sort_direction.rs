//! Which way one ordering term sorts (clause 4.23.1).

use crate::ordering::ordering_direction::OrderingDirection;

/// The two directions a sort key can run in.
///
/// Clause 4.23.1 offers four sort orders, but `dist-asc` and `dist-desc` differ from `asc` and `desc` only in measuring distance rather than value. Backends build every sort key from this reduction, and an ordering that admits no distance term (the temporal one of clause 5.7.4.3) carries it rather than the four-way direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SortDirection {
    /// Smallest first.
    Ascending,
    /// Largest first.
    Descending,
}

impl From<OrderingDirection> for SortDirection {
    fn from(direction: OrderingDirection) -> SortDirection {
        match direction {
            OrderingDirection::Asc | OrderingDirection::DistAsc => SortDirection::Ascending,
            OrderingDirection::Desc | OrderingDirection::DistDesc => SortDirection::Descending,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::ordering::{ordering_direction::OrderingDirection, sort_direction::SortDirection};
    use pretty_assertions::assert_eq;

    /// Clause 4.23.1's four sort orders differ in what they measure, not in how
    /// many directions there are.
    #[test]
    fn a_distance_direction_sorts_the_same_way_its_value_counterpart_does() {
        assert_eq!(SortDirection::from(OrderingDirection::Asc), SortDirection::Ascending);
        assert_eq!(SortDirection::from(OrderingDirection::DistAsc), SortDirection::Ascending);
        assert_eq!(SortDirection::from(OrderingDirection::Desc), SortDirection::Descending);
        assert_eq!(SortDirection::from(OrderingDirection::DistDesc), SortDirection::Descending);
    }
}
