//! The page sizes an instance-returning temporal read runs under.

use std::num::NonZeroU64;

/// The broker's clause 6.3.10 limits on the instances one temporal read
/// returns per attribute.
///
/// Built from a validated [`TemporalConfig`](andromeda_configuration::schema::temporal::TemporalConfig), so
/// `default_page` is at least one and never above `maximum`, and `edge_group`
/// never below it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TemporalInstanceLimits {
    /// Instances per attribute a read without `lastN` returns before it is
    /// paged forwards.
    pub default_page: u32,
    /// The largest `lastN` answered in full, and the periods per attribute one
    /// page of an aggregated read holds.
    pub maximum: u32,
    /// The most instances of one attribute sharing the instant at a page edge
    /// one page carries; a page carries such a group whole, past its size if
    /// need be.
    pub edge_group: u32,
    /// The raw instances per attribute one page of an aggregated read
    /// aggregates at most, unless its first period alone holds more.
    pub aggregation_instances: NonZeroU64,
}
