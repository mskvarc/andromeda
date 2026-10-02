//! The bounds one temporal service applies to what a request may ask of it.

use crate::instance_limits::TemporalInstanceLimits;
use andromeda_ngsi_ld_representation::payload_budget::EntityPayloadBudget;

/// The bounds a temporal service applies to its reads and writes.
#[derive(Clone, Copy, Debug)]
pub struct TemporalServiceLimits {
    /// The clause 6.3.10 page sizes of instance-returning reads.
    pub instance_limits: TemporalInstanceLimits,
    /// The maximum history instances one delete-by-query may remove before it
    /// is refused with `TooManyResults` (the `temporal_delete_by_query`
    /// extension).
    pub delete_by_query_max_matches: u64,
    /// The maximum distinct aggregation groups one grouped aggregation may
    /// produce before it is refused with `TooManyResults` (the
    /// `temporal_aggregation_grouping` extension).
    pub aggregation_grouping_max_groups: u64,
    /// The budget a written temporal payload is admitted under before it is
    /// expanded, the one the Core API admits an entity under.
    pub payload_budget: EntityPayloadBudget,
}
