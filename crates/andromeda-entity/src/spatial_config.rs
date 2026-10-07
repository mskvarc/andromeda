//! Entity spatial-index consistency configuration.

use andromeda_configuration_scalars::human_duration::HumanDuration;
use serde::{Deserialize, Serialize};
use strum::{Display, EnumString};
use thiserror::Error;
use toml_comment::TomlComment;

/// Invalid spatial-index configuration.
#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum SpatialConfigError {
    /// Rebuild cadence was configured as zero.
    #[error("spatial.rebuild_interval must be greater than zero")]
    ZeroRebuildInterval,
}

/// How the resident spatial index is kept consistent with the store whose
/// geo queries it accelerates.
///
/// Clauses 4.10 and 5.7.2 define a geo query's answer by the geometries the
/// entities hold, so an index may narrow a query only while it holds every
/// entity a geometry could match. The three modes differ in how they uphold
/// that, and in the deployments they uphold it for.
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, Display, EnumString, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case")]
pub enum SpatialConsistency {
    /// The index is caught up from the store before it narrows a query.
    ///
    /// Before narrowing, the query path reads the location of every entity
    /// the store changed since the index's recorded position in the store's
    /// commit history, then advances the position. Exact in every deployment,
    /// including replicas sharing the store, at the cost of one indexed read
    /// per geo query; the default because a broker cannot know whether another
    /// process writes its database.
    #[default]
    Watermark,
    /// Every committed write updates the index before the write returns.
    ///
    /// Exact only when exactly one broker process writes the store, which then
    /// saves the per-query read `Watermark` makes. A write committed by another
    /// process sharing the store, a replica, reaches this process's index
    /// only at the next rebuild, and until then a geo query here can miss the
    /// entity.
    CommitSynchronous,
    /// The index is not kept: the store evaluates every geo query.
    ///
    /// Exact in every deployment, at the cost of the store evaluating the
    /// relation against every candidate row its own spatial index yields.
    StoreVerified,
}

/// Spatial-index consistency and rebuild policy.
#[derive(Deserialize, Serialize, TomlComment, Debug, Clone)]
pub struct SpatialConfig {
    /// How the index is kept consistent with the store: `watermark`
    /// (default), `commit-synchronous` or `store-verified`.
    #[toml_comment(inline)]
    pub consistency: SpatialConsistency,
    /// Interval between full index rebuilds from the store, in human-readable
    /// form (`"30m"`).
    ///
    /// The first rebuild runs when the tenant runtime starts; until it
    /// completes the store evaluates every geo query. Later rebuilds drop the
    /// entries of entities deleted since, and under `commit-synchronous` they
    /// are also what brings in a replica's writes.
    #[toml_comment(inline)]
    pub rebuild_interval: HumanDuration,
}

impl Default for SpatialConfig {
    fn default() -> SpatialConfig {
        SpatialConfig {
            consistency: SpatialConsistency::default(),
            rebuild_interval: HumanDuration::from_secs(1800),
        }
    }
}

impl SpatialConfig {
    /// Validate the spatial-index configuration.
    ///
    /// # Errors
    ///
    /// Reports [`SpatialConfigError::ZeroRebuildInterval`] when the rebuild
    /// cadence is configured as zero.
    pub const fn validate(&self) -> Result<(), SpatialConfigError> {
        if self.rebuild_interval.is_zero() {
            return Err(SpatialConfigError::ZeroRebuildInterval);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::spatial_config::{SpatialConfig, SpatialConfigError, SpatialConsistency};
    use andromeda_configuration_scalars::human_duration::HumanDuration;
    use pretty_assertions::assert_eq;
    use std::str::FromStr;

    #[test]
    fn a_zero_rebuild_interval_is_rejected_by_the_owner() {
        let config = SpatialConfig {
            consistency: SpatialConsistency::CommitSynchronous,
            rebuild_interval: HumanDuration::from_secs(0),
        };
        assert_eq!(config.validate(), Err(SpatialConfigError::ZeroRebuildInterval));
    }

    /// The default must be exact whether or not another broker process writes
    /// the same database, which only `watermark` and `store-verified` are, and
    /// of the two only `watermark` keeps the index.
    #[test]
    fn the_default_consistency_catches_the_index_up_from_the_store() {
        assert_eq!(SpatialConfig::default().consistency, SpatialConsistency::Watermark);
        assert_eq!(SpatialConsistency::default(), SpatialConsistency::Watermark);
    }

    #[test]
    fn every_consistency_mode_is_spelled_in_kebab_case() {
        for (spelling, mode) in [
            ("commit-synchronous", SpatialConsistency::CommitSynchronous),
            ("store-verified", SpatialConsistency::StoreVerified),
            ("watermark", SpatialConsistency::Watermark),
        ] {
            assert_eq!(SpatialConsistency::from_str(spelling), Ok(mode));
            assert_eq!(mode.to_string(), spelling);
            assert_eq!(serde_json::to_value(mode).expect("serialisable"), serde_json::Value::from(spelling));
        }
    }
}
