//! The `[entity.spatial_index]` table: how the resident spatial index is kept
//! consistent with the store.

use crate::human_duration::HumanDuration;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use strum::{Display, EnumString};

/// How the resident spatial index is kept consistent with the store whose
/// geo queries it accelerates.
///
/// Clauses 4.10 and 5.7.2 define a geo query's answer by the geometries the
/// entities hold, so an index may narrow a query only while it holds every
/// entity a geometry could match. The three modes differ in how they uphold
/// that, and in the deployments they uphold it for.
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, Display, EnumString, PartialEq, Eq, JsonSchema)]
#[schemars(description = "How the in-memory spatial index stays consistent with the database whose geo queries it narrows.")]
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
    #[schemars(
        description = "Before narrowing a geo query, catch the index up with every location the database changed since its last recorded position. Exact in every deployment, including replicas sharing the database, at the cost of one indexed read per geo query."
    )]
    Watermark,
    /// Every committed write updates the index before the write returns.
    ///
    /// Exact only when exactly one broker process writes the store, which then
    /// saves the per-query read `Watermark` makes. A write committed by another
    /// process sharing the store, a replica, reaches this process's index
    /// only at the next rebuild, and until then a geo query here can miss the
    /// entity.
    #[schemars(
        description = "Update the index with every committed write before the write returns. Exact only when a single broker process writes the database: another process's writes reach this index only at the next rebuild, and until then a geo query here can miss the entity."
    )]
    CommitSynchronous,
    /// The index is not kept: the store evaluates every geo query.
    ///
    /// Exact in every deployment, at the cost of the store evaluating the
    /// relation against every candidate row its own spatial index yields.
    #[schemars(
        description = "Keep no index and let the database evaluate every geo query. Exact in every deployment, at the cost of the database checking every candidate row its own spatial index yields."
    )]
    StoreVerified,
}

/// Spatial-index consistency and rebuild policy.
#[derive(Deserialize, Serialize, JsonSchema, Debug, Clone)]
#[schemars(description = "How each tenant's in-memory spatial index is kept consistent and rebuilt.")]
#[serde(deny_unknown_fields)]
pub struct SpatialIndexConfig {
    /// How the index is kept consistent with the store: `watermark`
    /// (default), `commit-synchronous` or `store-verified`.
    #[schemars(
        description = "How the index is kept consistent with the database. `\"commit-synchronous\"` saves a read per geo query but is exact only when a single broker process writes the database."
    )]
    pub consistency: SpatialConsistency,
    /// Interval between full index rebuilds from the store, in human-readable
    /// form (`"30m"`).
    ///
    /// The first rebuild runs when the tenant runtime starts; until it
    /// completes the store evaluates every geo query. Later rebuilds drop the
    /// entries of entities deleted since, and under `commit-synchronous` they
    /// are also what brings in a replica's writes.
    #[schemars(
        description = "How often each tenant's index is rebuilt from the database. The first rebuild runs when the tenant is loaded, and until it completes the database evaluates every geo query. Later rebuilds drop entities deleted since and, under `\"commit-synchronous\"`, bring in writes from other broker processes."
    )]
    pub rebuild_interval: HumanDuration,
}

impl Default for SpatialIndexConfig {
    fn default() -> SpatialIndexConfig {
        SpatialIndexConfig {
            consistency: SpatialConsistency::default(),
            rebuild_interval: HumanDuration::from_secs(1800),
        }
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::schema::entity::spatial_index::{SpatialConsistency, SpatialIndexConfig};
    use pretty_assertions::assert_eq;
    use std::str::FromStr;

    /// The default must be exact whether or not another broker process writes
    /// the same database, which only `watermark` and `store-verified` are, and
    /// of the two only `watermark` keeps the index.
    #[test]
    fn the_default_consistency_catches_the_index_up_from_the_store() {
        assert_eq!(SpatialIndexConfig::default().consistency, SpatialConsistency::Watermark);
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
