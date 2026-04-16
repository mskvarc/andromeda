//! Runtime capability probe for TimescaleDB TSL (Enterprise) features.
//!
//! Some features -- notably continuous aggregates (`add_continuous_aggregate_policy`)
//! -- are part of the TSL license and are not present on pure Apache builds.
//! Migrations probe for these and branch accordingly; the query builder then
//! needs the same knowledge at runtime so it can route aggregation queries to
//! the pre-materialised views when available and fall back to `attribute_history`
//! otherwise.
//!
//! The probe is performed once at repository startup and cached for the
//! process lifetime -- the capability set cannot change without a DB restart.

use sqlx::postgres::PgPool;

/// Feature flags for the attached TimescaleDB instance.
///
/// Populated at process startup by [`TimescaleFeatures::detect`].
#[derive(Debug, Clone, Copy)]
pub struct TimescaleFeatures {
    /// `add_continuous_aggregate_policy` / continuous aggregate DDL is available.
    ///
    /// When true, aggregation queries with bucket size `PT1H` or `PT1D` and
    /// methods in {Avg, Min, Max, Sum, TotalCount} can be routed to
    /// `attr_agg_1h` / `attr_agg_1d`.
    pub has_continuous_aggregates: bool,
}

impl TimescaleFeatures {
    /// Probe the database for TSL-only features.
    ///
    /// Mirrors the columnstore-vs-legacy-compression detection performed in
    /// `migrations/tenant/0008_attribute_history.sql`: checks whether a
    /// TSL-only function is registered in `pg_proc`.
    pub async fn detect(pool: &PgPool) -> Result<TimescaleFeatures, sqlx::Error> {
        let has_caggs: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM pg_proc WHERE proname = 'add_continuous_aggregate_policy')")
            .fetch_one(pool)
            .await?;
        Ok(TimescaleFeatures {
            has_continuous_aggregates: has_caggs,
        })
    }

    /// Empty feature set -- safe default when no TSL features are assumed.
    pub const fn empty() -> TimescaleFeatures {
        TimescaleFeatures {
            has_continuous_aggregates: false,
        }
    }
}
