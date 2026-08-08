//! The temporal window embedded in a clause-5.2.23 query.

use andromeda_ngsi_ld_lexicon::{aggr_method::AggrMethod, rfc3339, time_prop::TimeProp, time_rel::TimeRel};
use andromeda_ngsi_ld_query::temporal::aggregation_period::{self, AggregationPeriod};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_with::{DisplayFromStr, StringWithSeparator, formats::CommaSeparator, serde_as};

/// Temporal query parameters (ETSI GS CIM 009 v1.9.1 clause 5.2.21).
#[serde_as]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TemporalQuery {
    /// Reference instant for the temporal relation.
    #[serde(with = "rfc3339::datetime")]
    pub time_at: DateTime<Utc>,
    /// Relation between matching instances and the supplied instants.
    #[serde_as(as = "DisplayFromStr")]
    pub timerel: TimeRel,
    /// Aggregation methods applied to matching instances.
    #[serde_as(as = "Option<StringWithSeparator::<CommaSeparator, AggrMethod>>")]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aggr_methods: Option<Vec<AggrMethod>>,
    /// Width of each aggregation period.
    #[serde(with = "aggregation_period::optional", default, skip_serializing_if = "Option::is_none")]
    pub aggr_period_duration: Option<AggregationPeriod>,
    /// Closing instant, required for `between`.
    #[serde(with = "rfc3339::optional_datetime", default, skip_serializing_if = "Option::is_none")]
    pub end_time_at: Option<DateTime<Utc>>,
    /// Maximum matching instances returned per attribute.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_n: Option<u32>,
    /// Temporal property used for comparison.
    #[serde_as(as = "Option<DisplayFromStr>")]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeproperty: Option<TimeProp>,
}
