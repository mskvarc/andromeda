//! The tables of the extensions that refuse a request past a count.

use crate::schema::feature_mode::FeatureMode;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::num::NonZeroU64;

/// The temporal delete-by-query extension.
#[derive(Deserialize, Serialize, JsonSchema, Debug, Clone, Copy, PartialEq, Eq)]
#[schemars(deny_unknown_fields)]
#[schemars(description = "Temporal delete-by-query: a windowed, selector-scoped purge of temporal history on `DELETE /temporal/entities`.")]
pub struct TemporalDeleteByQueryConfig {
    /// Whether the extension is served: `enabled` or `disabled`.
    #[schemars(
        description = "Whether temporal delete-by-query is served on `DELETE /temporal/entities`. It acts on local history only and is never forwarded."
    )]
    pub mode: FeatureMode,
    /// The most history instances one delete-by-query removes before it is
    /// refused with `TooManyResults` (403). Counted before anything is
    /// deleted, so an oversized purge is refused whole rather than
    /// half-executed.
    #[schemars(
        description = "The most history instances one delete-by-query may remove. The matches are counted before anything is deleted, so a larger purge is refused whole with `403` `TooManyResults` and deletes nothing."
    )]
    pub max_matches: NonZeroU64,
}

impl Default for TemporalDeleteByQueryConfig {
    fn default() -> TemporalDeleteByQueryConfig {
        TemporalDeleteByQueryConfig {
            mode: FeatureMode::Disabled,
            max_matches: NonZeroU64::MIN.saturating_add(99_999),
        }
    }
}

/// The grouped temporal aggregation extension.
#[derive(Deserialize, Serialize, JsonSchema, Debug, Clone, Copy, PartialEq, Eq)]
#[schemars(deny_unknown_fields)]
#[schemars(description = "Grouped temporal aggregation through `aggrGroupBy` and `format=groupedValues`.")]
pub struct TemporalAggregationGroupingConfig {
    /// Whether the extension is served: `enabled` or `disabled`.
    #[schemars(
        description = "Whether grouped temporal aggregation is served: calendar and cyclic time buckets, per-entity-member keys and value-in-effect keys, with groups of mixed units split rather than refused. Off, `aggrGroupBy` and `groupedValues` are refused with `InvalidRequest`."
    )]
    pub mode: FeatureMode,
    /// The most distinct groups one grouped aggregation produces before it is
    /// refused with `TooManyResults` (403). Counted before the groups are
    /// computed, so an oversized grouping is refused whole rather than
    /// partly returned.
    #[schemars(
        description = "The most distinct groups one grouped aggregation may produce. The groups are counted before they are computed, so a larger grouping is refused whole with `403` `TooManyResults`."
    )]
    pub max_groups: NonZeroU64,
}

impl Default for TemporalAggregationGroupingConfig {
    fn default() -> TemporalAggregationGroupingConfig {
        TemporalAggregationGroupingConfig {
            mode: FeatureMode::Disabled,
            max_groups: NonZeroU64::MIN.saturating_add(9_999),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::schema::{
        extensions::bounded::{TemporalAggregationGroupingConfig, TemporalDeleteByQueryConfig},
        feature_mode::FeatureMode,
    };
    use pretty_assertions::assert_eq;

    #[test]
    fn both_are_off_by_default_with_their_documented_bounds() {
        let delete = TemporalDeleteByQueryConfig::default();
        assert_eq!((delete.mode, delete.max_matches.get()), (FeatureMode::Disabled, 100_000));
        let grouping = TemporalAggregationGroupingConfig::default();
        assert_eq!((grouping.mode, grouping.max_groups.get()), (FeatureMode::Disabled, 10_000));
    }

    /// A zero bound refuses every request the extension could serve, which is
    /// disabling it with the wrong error shape, so it is refused as it is
    /// read.
    #[test]
    fn a_zero_bound_is_refused_as_it_is_read() {
        assert!(toml::from_str::<TemporalDeleteByQueryConfig>("mode = \"enabled\"\nmax_matches = 0\n").is_err());
        assert!(toml::from_str::<TemporalAggregationGroupingConfig>("mode = \"enabled\"\nmax_groups = 0\n").is_err());
    }
}
