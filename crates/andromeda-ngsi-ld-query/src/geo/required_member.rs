//! The members of a geo-query that Table 5.2.13-1 gives cardinality 1.

use strum::Display;

/// The members Table 5.2.13-1 gives cardinality 1.
#[derive(Clone, Copy, Debug, Display, PartialEq, Eq)]
#[strum(serialize_all = "lowercase")]
pub enum RequiredMember {
    /// `georel`.
    Georel,
    /// `geometry`.
    Geometry,
    /// `coordinates`.
    Coordinates,
}
