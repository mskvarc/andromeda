//! The page sizes an instance-returning temporal read runs under.

/// The broker's clause 6.3.10 limits on the instances one temporal read
/// returns per attribute.
///
/// Built from a validated [`TemporalConfig`](crate::config::TemporalConfig), so
/// `default_page` is at least one and never above `maximum`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TemporalInstanceLimits {
    /// Instances per attribute a read without `lastN` returns before it is
    /// paged forwards.
    pub default_page: u32,
    /// The largest `lastN` answered in full, and the largest group of instances
    /// sharing one timestamp a page may carry at its edge.
    pub maximum: u32,
}
