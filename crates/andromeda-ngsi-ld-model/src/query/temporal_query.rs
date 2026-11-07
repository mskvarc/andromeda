//! The temporal window embedded in a clause-5.2.23 query.

use andromeda_ngsi_ld_lexicon::{aggr_method::AggrMethod, time::instant_serde, time_prop::TimeProp, time_rel::TimeRel};
use andromeda_ngsi_ld_query::temporal::aggregation_period::{self, AggregationPeriod};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_with::{DisplayFromStr, StringWithSeparator, formats::CommaSeparator, serde_as};
use std::num::NonZeroU32;

/// Temporal query parameters (ETSI GS CIM 009 v1.9.1 clause 5.2.21).
#[serde_as]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TemporalQuery {
    /// Reference instant for the temporal relation.
    #[serde(with = "instant_serde::required")]
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
    #[serde(with = "instant_serde::optional", default, skip_serializing_if = "Option::is_none")]
    pub end_time_at: Option<DateTime<Utc>>,
    /// Maximum matching instances returned per attribute, a "Positive integer"
    /// (Table 5.2.21-1), so `0` fails to decode.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_n: Option<NonZeroU32>,
    /// Temporal property used for comparison.
    #[serde_as(as = "Option<DisplayFromStr>")]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeproperty: Option<TimeProp>,
}

#[cfg(test)]
mod tests {
    use crate::query::temporal_query::TemporalQuery;
    use std::num::NonZeroU32;

    /// Table 5.2.21-1 makes `lastN` a "Positive integer".
    #[test]
    fn a_zero_last_n_is_refused() {
        let decoded = serde_json::from_str::<TemporalQuery>(r#"{"timerel":"after","timeAt":"2024-01-01T00:00:00Z","lastN":0}"#);

        assert!(decoded.is_err(), "{decoded:?}");
    }

    #[test]
    fn a_positive_last_n_is_kept() {
        let decoded =
            serde_json::from_str::<TemporalQuery>(r#"{"timerel":"after","timeAt":"2024-01-01T00:00:00Z","lastN":1}"#).expect("a valid temporal query");

        pretty_assertions::assert_eq!(decoded.last_n.map(NonZeroU32::get), Some(1));
    }

    /// Table 5.2.21-1: `timeAt` and `timerel` have cardinality 1, `timerel`
    /// takes "before", "after" or "between", `timeAt` and `endTimeAt` are
    /// each a date-time, and `timeproperty` names a temporal property.
    #[test]
    fn a_temporal_query_breaking_its_data_type_restriction_is_refused() {
        for body in [
            r#"{"timerel":"after"}"#,
            r#"{"timeAt":"2024-01-01T00:00:00Z"}"#,
            r#"{"timerel":"during","timeAt":"2024-01-01T00:00:00Z"}"#,
            r#"{"timerel":"after","timeAt":"yesterday"}"#,
            r#"{"timerel":"between","timeAt":"2024-01-01T00:00:00Z","endTimeAt":"tomorrow"}"#,
            r#"{"timerel":"after","timeAt":"2024-01-01T00:00:00Z","timeproperty":"expiresAt"}"#,
            r#"{"timerel":"after","timeAt":"2024-01-01T00:00:00Z","lastN":-1}"#,
        ] {
            assert!(serde_json::from_str::<TemporalQuery>(body).is_err(), "{body}");
        }
    }
}
