//! What one page of an aggregated temporal answer may hold per attribute
//! stream (clauses 4.5.19 and 6.3.10).

use std::num::NonZeroU64;

/// The bounds a page of whole aggregation periods is read under, per attribute
/// stream.
///
/// A period is the element of the aggregated representation as an instance is
/// of the normalized one (clause 4.5.19), so a page counts periods. The raw
/// instances a page aggregates are bounded too, since a coarse period over
/// dense history holds any number of them. A page always holds at least one
/// whole period whatever it holds: an aggregate over part of a period is a
/// different number, so a period is never split.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PeriodBudget {
    /// The periods a page holds at most.
    pub periods: u32,
    /// The raw instances a page aggregates at most, unless its first period
    /// alone holds more.
    pub instances: NonZeroU64,
}
