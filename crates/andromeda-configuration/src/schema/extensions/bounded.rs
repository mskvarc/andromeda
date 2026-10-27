//! The tables of the extensions that refuse a request past a count.

use crate::schema::feature_mode::FeatureMode;
use serde::{Deserialize, Serialize};
use std::num::NonZeroU64;
use toml_comment::TomlComment;

/// The temporal delete-by-query extension.
#[derive(Deserialize, Serialize, TomlComment, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TemporalDeleteByQueryConfig {
    /// Whether the extension is served: `enabled` or `disabled`.
    #[toml_comment(inline)]
    pub mode: FeatureMode,
    /// The most history instances one delete-by-query removes before it is
    /// refused with `TooManyResults` (403). Counted before anything is
    /// deleted, so an oversized purge is refused whole rather than
    /// half-executed.
    #[toml_comment(inline)]
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
#[derive(Deserialize, Serialize, TomlComment, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TemporalAggregationGroupingConfig {
    /// Whether the extension is served: `enabled` or `disabled`.
    #[toml_comment(inline)]
    pub mode: FeatureMode,
    /// The most distinct groups one grouped aggregation produces before it is
    /// refused with `TooManyResults` (403). Counted before the groups are
    /// computed, so an oversized grouping is refused whole rather than
    /// partly returned.
    #[toml_comment(inline)]
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
