//! Reconciling the temporal hypertables' physical storage policy with the
//! broker's configuration.
//!
//! The migrations declare a default chunk interval for `attribute_history`,
//! and nothing about the columnstore: on an edition that has
//! one, [`crate::columnstore_enablement`] enables it when a tenant is opened.
//! The chunk interval and the columnstore lag are operator tunables, and a
//! tunable that needed a migration to change would not be one, so when a tenant
//! is opened the stored values are read back from the TimescaleDB information
//! views and moved to what the configuration names — only when they differ, so
//! an unchanged configuration is a read and no write.

use crate::{
    columnstore_capability::ColumnstoreSupport,
    columnstore_enablement::reconcile_columnstore_enablement,
    error::TemporalStoreError,
    repo::TemporalRepository,
    schema_ddl::timescale_policy::{HypertableName, add_columnstore_policy, remove_columnstore_policy},
    temporal_store_invariant::TemporalStoreInvariant,
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
    /// The value has no bearing on this edition: the Apache-2.0 core holds no
    /// columnstore, so neither it nor its lag is read or written. Only the
    /// columnstore axes ever report this; the chunk interval is core surface.
    Unsupported,
}

impl PolicyReconciliation {
    /// Whether reconciling wrote nothing to the server.
    ///
    /// Both a value that already matched and one the edition cannot hold left
    /// the database untouched, so both count as no write; only [`Adjusted`]
    /// issued a statement.
    ///
    /// [`Adjusted`]: PolicyReconciliation::Adjusted
    const fn wrote_nothing(self) -> bool {
        match self {
            PolicyReconciliation::Unchanged | PolicyReconciliation::Unsupported => true,
            PolicyReconciliation::Adjusted => false,
        }
    }
}

/// What reconciling one hypertable did.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HypertableReconciliation {
    /// The hypertable reconciled.
    pub hypertable: HypertableName,
    /// What happened to its chunk interval.
    pub chunk_interval: PolicyReconciliation,
    /// Whether its columnstore was enabled.
    pub columnstore_enabled: PolicyReconciliation,
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
    /// Whether nothing was written: every stored value already matched, or the
    /// edition holds no columnstore to enable or move.
    #[must_use]
    pub fn is_unchanged(&self) -> bool {
        self.hypertables.iter().all(|reconciliation| {
            reconciliation.chunk_interval.wrote_nothing()
                && reconciliation.columnstore_enabled.wrote_nothing()
                && reconciliation.columnstore_after.wrote_nothing()
        })
    }
}

impl TemporalRepository {
    /// Move every temporal hypertable's chunk interval to what `policy` names,
    /// leaving it untouched when the policy is silent on it, and -- on an
    /// edition with a columnstore -- enable the columnstore and move its lag to
    /// the policy's.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalStoreError::Database`] when an information view cannot
    /// be read or a policy statement fails, [`TemporalStoreError::Invariant`] when
    /// a stored interval does not decode, and
    /// [`TemporalStoreError::ColumnstoreSettingsDrift`] when a hypertable
    /// already carries the columnstore under other settings.
    pub async fn reconcile_storage_policy(&self, policy: &TemporalStoragePolicy) -> Result<TemporalStoragePolicyOutcome, TemporalStoreError> {
        let mut hypertables = Vec::new();
        for hypertable in HypertableName::iter() {
            let chunk_interval = match policy.chunk_interval_days {
                Some(days) => reconcile_chunk_interval(&self.pool, hypertable, days).await?,
                None => PolicyReconciliation::Unchanged,
            };
            // The lag is a policy on the columnstore, so the columnstore is
            // enabled first: a policy on a hypertable without one is refused.
            let (columnstore_enabled, columnstore_after) = match plan_columnstore_reconciliation(self.columnstore, policy.columnstore_after_days) {
                ColumnstoreReconciliation::Unsupported => (PolicyReconciliation::Unsupported, PolicyReconciliation::Unsupported),
                ColumnstoreReconciliation::Reconcile(days) => (
                    reconcile_columnstore_enablement(&self.pool, hypertable).await?,
                    reconcile_columnstore_lag(&self.pool, hypertable, days).await?,
                ),
            };
            hypertables.push(HypertableReconciliation {
                hypertable,
                chunk_interval,
                columnstore_enabled,
                columnstore_after,
            });
        }
        Ok(TemporalStoragePolicyOutcome { hypertables })
    }
}

/// What the columnstore asks of one hypertable, once the edition's capability
/// is taken into account.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ColumnstoreReconciliation {
    /// The edition holds no columnstore: nothing is read or written.
    Unsupported,
    /// The columnstore is to be enabled, and its stored lag compared against,
    /// and moved to, this configured day count.
    Reconcile(NonZeroU32),
}

/// Decide what the columnstore means for one hypertable before any statement
/// runs: [`Unsupported`](ColumnstoreReconciliation::Unsupported) on the
/// Apache-2.0 core, whatever the configuration names, and reconciled against
/// the configured lag on the TSL edition.
const fn plan_columnstore_reconciliation(support: ColumnstoreSupport, configured: NonZeroU32) -> ColumnstoreReconciliation {
    match support {
        ColumnstoreSupport::Absent => ColumnstoreReconciliation::Unsupported,
        ColumnstoreSupport::Present => ColumnstoreReconciliation::Reconcile(configured),
    }
}

/// Set the chunk interval of `hypertable` to `days` unless it already is.
async fn reconcile_chunk_interval(pool: &PgPool, hypertable: HypertableName, days: NonZeroU32) -> Result<PolicyReconciliation, TemporalStoreError> {
    let (sql, values) = chunk_interval_statement(hypertable.into()).build_sqlx(PostgresQueryBuilder);
    let stored: Option<PgInterval> = sqlx::query_scalar_with(AssertSqlSafe(sql), values).fetch_optional(pool).await?;
    let stored = stored.ok_or(TemporalStoreInvariant::NoTimeDimension(hypertable))?;
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
        columnstore_capability::ColumnstoreSupport,
        schema_ddl::timescale_policy::HypertableName,
        storage_policy::{
            ColumnstoreReconciliation,
            HypertableReconciliation,
            PolicyReconciliation,
            TemporalStoragePolicyOutcome,
            day_seconds,
            interval_seconds,
            plan_columnstore_reconciliation,
        },
    };
    use pretty_assertions::assert_eq;
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
                columnstore_enabled: PolicyReconciliation::Unchanged,
                columnstore_after: PolicyReconciliation::Unchanged,
            }],
        };
        assert!(untouched.is_unchanged());
        let adjusted = TemporalStoragePolicyOutcome {
            hypertables: vec![HypertableReconciliation {
                hypertable: HypertableName::AttributeHistory,
                chunk_interval: PolicyReconciliation::Unchanged,
                columnstore_enabled: PolicyReconciliation::Unchanged,
                columnstore_after: PolicyReconciliation::Adjusted,
            }],
        };
        assert!(!adjusted.is_unchanged());
    }

    #[test]
    fn an_unsupported_columnstore_over_an_unchanged_chunk_interval_still_wrote_nothing() {
        let apache = TemporalStoragePolicyOutcome {
            hypertables: vec![HypertableReconciliation {
                hypertable: HypertableName::AttributeHistory,
                chunk_interval: PolicyReconciliation::Unchanged,
                columnstore_enabled: PolicyReconciliation::Unsupported,
                columnstore_after: PolicyReconciliation::Unsupported,
            }],
        };
        assert!(apache.is_unchanged(), "the Apache core holds no columnstore to enable or move");
        let apache_with_adjusted_interval = TemporalStoragePolicyOutcome {
            hypertables: vec![HypertableReconciliation {
                hypertable: HypertableName::AttributeHistory,
                chunk_interval: PolicyReconciliation::Adjusted,
                columnstore_enabled: PolicyReconciliation::Unsupported,
                columnstore_after: PolicyReconciliation::Unsupported,
            }],
        };
        assert!(!apache_with_adjusted_interval.is_unchanged(), "the Apache-safe chunk interval was still moved");
    }

    #[test]
    fn the_apache_edition_resolves_the_columnstore_to_unsupported_without_a_statement() {
        let three_days = NonZeroU32::new(3).expect("positive");
        assert_eq!(
            plan_columnstore_reconciliation(ColumnstoreSupport::Absent, three_days),
            ColumnstoreReconciliation::Unsupported,
            "an Apache tenant issues no columnstore statement whatever lag is configured"
        );
    }

    #[test]
    fn the_tsl_edition_reconciles_the_configured_lag() {
        let five_days = NonZeroU32::new(5).expect("positive");
        assert_eq!(
            plan_columnstore_reconciliation(ColumnstoreSupport::Present, five_days),
            ColumnstoreReconciliation::Reconcile(five_days),
            "the columnstore is enabled and its lag reconciled against the stored policy"
        );
    }

    #[test]
    fn enabling_the_columnstore_is_a_write() {
        let enabled = TemporalStoragePolicyOutcome {
            hypertables: vec![HypertableReconciliation {
                hypertable: HypertableName::AttributeHistory,
                chunk_interval: PolicyReconciliation::Unchanged,
                columnstore_enabled: PolicyReconciliation::Adjusted,
                columnstore_after: PolicyReconciliation::Unchanged,
            }],
        };
        assert!(!enabled.is_unchanged());
    }
}
