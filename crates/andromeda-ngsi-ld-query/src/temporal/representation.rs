//! How a temporal query renders the instances it selected.

use crate::temporal::aggregation_period::AggregationPeriod;
use andromeda_ngsi_ld_lexicon::{aggr_method::AggrMethod, temporal_format::TemporalFormat};
use andromeda_ngsi_ld_model::sys_attrs::SysAttrsInclusion;
use typed_builder::TypedBuilder;

/// The response shape of a temporal query.
///
/// Nothing here changes which rows are read — only what is done with them, which
/// is why it is separate from the selection types. `aggr_methods` and
/// `aggr_period_duration` are meaningful only when `format` is
/// [`TemporalFormat::AggregatedValues`] (clause 4.5.19).
#[derive(Debug, Clone, TypedBuilder)]
pub struct Representation {
    /// Normalized, `temporalValues` or `aggregatedValues`.
    pub format: TemporalFormat,
    /// Aggregation methods applied per bucket.
    #[builder(default)]
    pub aggr_methods: Vec<AggrMethod>,
    /// Aggregation bucket width, absent when the whole range is one bucket
    /// (clause 4.5.19).
    #[builder(default)]
    pub aggr_period_duration: Option<AggregationPeriod>,
    /// Whether `createdAt` / `modifiedAt` survive into the response.
    pub sys_attrs: SysAttrsInclusion,
}
