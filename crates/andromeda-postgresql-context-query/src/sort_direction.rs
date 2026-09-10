//! Which way one ordering term sorts.

use andromeda_ngsi_ld_query::ordering::term::OrderDirection;
use sea_query::Order;

/// The two directions a sort key can run in.
///
/// Clause 4.23.1 offers four sort orders, but two of them differ only in what
/// is measured: `dist-asc` and `dist-desc` sort by distance rather than by
/// value, ascending and descending exactly as `asc` and `desc` do. Every SQL
/// sort key is built from this reduction, so no key has to re-derive which of
/// the four term directions it is under.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

impl From<SortDirection> for Order {
    fn from(direction: SortDirection) -> Order {
        match direction {
            SortDirection::Ascending => Order::Asc,
            SortDirection::Descending => Order::Desc,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::sort_direction::SortDirection;
    use andromeda_ngsi_ld_query::ordering::term::OrderDirection;
    use sea_query::Order;

    /// Clause 4.23.1's four sort orders differ in what they measure, not in how
    /// many directions there are.
    #[test]
    fn a_distance_direction_sorts_the_same_way_its_value_counterpart_does() {
        assert_eq!(SortDirection::from(OrderDirection::Asc), SortDirection::Ascending);
        assert_eq!(SortDirection::from(OrderDirection::DistAsc), SortDirection::Ascending);
        assert_eq!(SortDirection::from(OrderDirection::Desc), SortDirection::Descending);
        assert_eq!(SortDirection::from(OrderDirection::DistDesc), SortDirection::Descending);
    }

    #[test]
    fn each_direction_renders_its_own_sql_order() {
        assert_eq!(Order::from(SortDirection::Ascending), Order::Asc);
        assert_eq!(Order::from(SortDirection::Descending), Order::Desc);
    }
}
