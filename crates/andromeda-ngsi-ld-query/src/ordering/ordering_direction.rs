//! The direction of an `orderBy` term (clause 4.23).

use strum::{Display, EnumString};

/// Sort direction for `orderBy` terms per NGSI-LD S4.23.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Display, EnumString)]
#[strum(serialize_all = "kebab-case")]
pub enum OrderingDirection {
    /// Sort in ascending order.
    Asc,
    /// Sort in descending order.
    Desc,
    /// Sort by distance in ascending order from the geo query reference geometry (S4.23).
    DistAsc,
    /// Sort by distance in descending order from the geo query reference geometry (S4.23).
    DistDesc,
}

impl OrderingDirection {
    /// Whether this is a distance-based ordering direction.
    #[must_use]
    pub const fn is_distance(&self) -> bool {
        matches!(self, OrderingDirection::DistAsc | OrderingDirection::DistDesc)
    }
}
