//! Temporal aggregation members carried by a clause-5.2.23 query.

use andromeda_ngsi_ld_lexicon::aggr_method::AggrMethod;
use andromeda_ngsi_ld_query::temporal::aggregation_period::{self, AggregationPeriod};
use serde::{Deserialize, Serialize};
use serde_with::{StringWithSeparator, formats::CommaSeparator, serde_as};

/// Aggregation parameters (ETSI GS CIM 009 v1.9.1 clause 5.2.44).
#[serde_as]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AggregationParameters {
    /// Methods applied to each aggregation period.
    #[serde_as(as = "StringWithSeparator::<CommaSeparator, AggrMethod>")]
    pub aggr_methods: Vec<AggrMethod>,
    /// Width of each aggregation period; absent means the full time range.
    #[serde(with = "aggregation_period::optional", default, skip_serializing_if = "Option::is_none")]
    pub aggr_period_duration: Option<AggregationPeriod>,
}
