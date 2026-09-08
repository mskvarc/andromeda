//! Reconciling the temporal hypertables' physical storage policy with the
//! broker's configuration.
//!
//! The migrations declare a default chunk interval and a default columnstore
//! lag for `attribute_history` and `scope_history`. Both are operator tunables,
//! and a tunable that needed a migration to change would not be one, so when a
//! tenant is opened the stored values are read back from the TimescaleDB
//! information views and moved to what the configuration names — only when
//! they differ, so an unchanged configuration is a read and no write.

use crate::{
    error::TemporalStoreError,
    repo::TemporalRepository,
    schema_ddl::timescale_policy::{HypertableName, add_columnstore_policy, remove_columnstore_policy},
};
use andromeda_postgresql_seaquery::timescale::{chunk_interval_statement, columnstore_policy_statement, set_chunk_time_interval};
use andromeda_temporal::storage_policy::TemporalStoragePolicy;
use sea_query::{Expr, PostgresQueryBuilder, Query};
use sea_query_sqlx::SqlxBinder;
use sqlx::{AssertSqlSafe, PgPool, postgres::types::PgInterval};
use std::num::NonZeroU32;
use strum::IntoEnumIterator;

/// Seconds in one day, the unit both policy values are configured in.
const SECONDS_PER_DAY: i64 = 86_400;

/// Days `TimescaleDB` counts one interval month as when it stores a policy.
const DAYS_PER_MONTH: i64 = 30;

/// Whether one stored policy value was already what the configuration names.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PolicyReconciliation {
    /// The stored value already matched, or the configuration names none.
    Unchanged,
    /// The stored value was moved to the configured one.
    Adjusted,
}

/// What reconciling one hypertable did.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HypertableReconciliation {
    /// The hypertable reconciled.
    pub hypertable: HypertableName,
    /// What happened to its chunk interval.
    pub chunk_interval: PolicyReconciliation,
    /// What happened to its columnstore lag.
    pub columnstore_after: PolicyReconciliation,
}

/// What reconciling every temporal hypertable did.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TemporalStoragePolicyOutcome {
    /// One entry per hypertable, in catalogue order.
    pub hypertables: Vec<HypertableReconciliation>,
}

impl TemporalStoragePolicyOutcome {
    /// Whether nothing was written: every stored value already matched.
    #[must_use]
    pub fn is_unchanged(&self) -> bool {
        self.hypertables.iter().all(|reconciliation| {
            reconciliation.chunk_interval == PolicyReconciliation::Unchanged && reconciliation.columnstore_after == PolicyReconciliation::Unchanged
        })
    }
}

impl TemporalRepository {
    /// Move every temporal hypertable's chunk interval and columnstore lag to
    /// what `policy` names, leaving values the policy is silent on untouched.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalStoreError::Database`] when an information view cannot
    /// be read or a policy statement fails, and [`TemporalStoreError::Decode`]
    /// when a stored interval does not decode.
    pub async fn reconcile_storage_policy(&self, policy: &TemporalStoragePolicy) -> Result<TemporalStoragePolicyOutcome, TemporalStoreError> {
        let mut hypertables = Vec::with_capacity(2);
        for hypertable in HypertableName::iter() {
            let chunk_interval = match policy.chunk_interval_days {
                Some(days) => reconcile_chunk_interval(&self.pool, hypertable, days).await?,
                None => PolicyReconciliation::Unchanged,
            };
            let columnstore_after = match policy.columnstore_after_days {
                Some(days) => reconcile_columnstore_lag(&self.pool, hypertable, days).await?,
                None => PolicyReconciliation::Unchanged,
            };
            hypertables.push(HypertableReconciliation {
                hypertable,
                chunk_interval,
                columnstore_after,
            });
        }
        Ok(TemporalStoragePolicyOutcome { hypertables })
    }
}

/// Set the chunk interval of `hypertable` to `days` unless it already is.
async fn reconcile_chunk_interval(pool: &PgPool, hypertable: HypertableName, days: NonZeroU32) -> Result<PolicyReconciliation, TemporalStoreError> {
    let (sql, values) = chunk_interval_statement(hypertable.into()).build_sqlx(PostgresQueryBuilder);
    let stored: Option<PgInterval> = sqlx::query_scalar_with(AssertSqlSafe(sql), values).fetch_optional(pool).await?;
    let stored = stored.ok_or_else(|| TemporalStoreError::Decode(format!("{hypertable} declares no time dimension to reconcile")))?;
    if interval_seconds(&stored) == day_seconds(days) {
        return Ok(PolicyReconciliation::Unchanged);
    }
    let (sql, values) = Query::select()
        .expr(set_chunk_time_interval(hypertable.into(), Expr::val(day_count(days))))
        .to_owned()
        .build_sqlx(PostgresQueryBuilder);
    sqlx::query_with(AssertSqlSafe(sql), values).execute(pool).await?;
    Ok(PolicyReconciliation::Adjusted)
}

/// Set the columnstore lag of `hypertable` to `days` unless it already is.
///
/// A policy's lag cannot be edited in place, so a differing one is removed and
/// re-added; a missing one is simply added.
async fn reconcile_columnstore_lag(pool: &PgPool, hypertable: HypertableName, days: NonZeroU32) -> Result<PolicyReconciliation, TemporalStoreError> {
    let (sql, values) = columnstore_policy_statement(hypertable.into()).build_sqlx(PostgresQueryBuilder);
    let stored: Option<PgInterval> = sqlx::query_scalar_with(AssertSqlSafe(sql), values).fetch_optional(pool).await?;
    if stored.is_some_and(|lag| interval_seconds(&lag) == day_seconds(days)) {
        return Ok(PolicyReconciliation::Unchanged);
    }
    if stored.is_some() {
        sqlx::query(AssertSqlSafe(remove_columnstore_policy(hypertable))).execute(pool).await?;
    }
    sqlx::query(AssertSqlSafe(add_columnstore_policy(hypertable, days))).execute(pool).await?;
    Ok(PolicyReconciliation::Adjusted)
}

/// The whole seconds an interval spans, counting a month as thirty days as
/// `TimescaleDB` does when it stores a day-denominated policy.
const fn interval_seconds(interval: &PgInterval) -> i64 {
    (interval.months as i64 * DAYS_PER_MONTH + interval.days as i64) * SECONDS_PER_DAY + interval.microseconds / 1_000_000
}

/// The seconds a configured day count spans.
const fn day_seconds(days: NonZeroU32) -> i64 {
    days.get() as i64 * SECONDS_PER_DAY
}

/// The day count as the `integer` `make_interval(days => …)` takes.
const fn day_count(days: NonZeroU32) -> i32 {
    // Configuration is a `u32`, so a value past `i32::MAX` days is not a
    // storage policy anyone can mean; it saturates rather than wraps.
    if days.get() > i32::MAX.cast_unsigned() {
        i32::MAX
    } else {
        days.get().cast_signed()
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        schema_ddl::timescale_policy::HypertableName,
        storage_policy::{HypertableReconciliation, PolicyReconciliation, TemporalStoragePolicyOutcome, day_seconds, interval_seconds},
    };
    use sqlx::postgres::types::PgInterval;
    use std::num::NonZeroU32;

    #[test]
    fn a_stored_interval_is_compared_in_whole_seconds_however_it_was_stored() {
        let seven_days = PgInterval {
            months: 0,
            days: 7,
            microseconds: 0,
        };
        let seven_days_in_micros = PgInterval {
            months: 0,
            days: 0,
            microseconds: 7 * 86_400 * 1_000_000,
        };
        let one_month = PgInterval {
            months: 1,
            days: 0,
            microseconds: 0,
        };
        assert_eq!(interval_seconds(&seven_days), day_seconds(NonZeroU32::new(7).expect("positive")));
        assert_eq!(interval_seconds(&seven_days_in_micros), day_seconds(NonZeroU32::new(7).expect("positive")));
        assert_eq!(interval_seconds(&one_month), day_seconds(NonZeroU32::new(30).expect("positive")));
    }

    #[test]
    fn an_outcome_is_unchanged_only_when_every_hypertable_was_left_alone() {
        let untouched = TemporalStoragePolicyOutcome {
            hypertables: vec![HypertableReconciliation {
                hypertable: HypertableName::AttributeHistory,
                chunk_interval: PolicyReconciliation::Unchanged,
                columnstore_after: PolicyReconciliation::Unchanged,
            }],
        };
        assert!(untouched.is_unchanged());
        let adjusted = TemporalStoragePolicyOutcome {
            hypertables: vec![HypertableReconciliation {
                hypertable: HypertableName::ScopeHistory,
                chunk_interval: PolicyReconciliation::Unchanged,
                columnstore_after: PolicyReconciliation::Adjusted,
            }],
        };
        assert!(!adjusted.is_unchanged());
    }
}
