//! Reconciling the temporal hypertables' physical storage policy with the
//! broker's configuration.
//!
//! The migrations declare a default chunk interval for `attribute_history`,
//! and nothing about the columnstore: on an edition that has
//! one, [`crate::columnstore_enablement`] enables it when a tenant is opened,
//! and [`crate::chunk_skipping_enablement`] registers the columns a converted
//! chunk keeps a range of.
//! The chunk interval is an operator tunable, and a tunable that needed a
//! migration to change would not be one, so when a tenant is opened the stored
//! value is read back from the TimescaleDB information views and moved to what
//! the configuration names, only when they differ, so an unchanged
//! configuration is a read and no write. Each write waits a bounded time for
//! its lock ([`crate::bounded_lock_wait`]).
//!
//! When chunks are converted is not stored in the database at all: the
//! broker's own sweep converts them past `temporal.columnstore_after`
//! ([`crate::columnstore_conversion`]).

use crate::{
    bounded_lock_wait::{StatementWrite, execute_with_bounded_lock_wait},
    chunk_skipping_enablement::reconcile_chunk_skipping,
    columnstore_capability::ColumnstoreSupport,
    columnstore_enablement::reconcile_columnstore_enablement,
    error::TemporalStoreError,
    hypertable_policy::HypertableName,
    repo::TemporalRepository,
    temporal_store_invariant::TemporalStoreInvariant,
};
use andromeda_postgresql_seaquery::{
    statement::Statement,
    timescale::{chunk_interval_statement, set_chunk_time_interval},
};
use andromeda_temporal::storage_policy::TemporalStoragePolicy;
use sea_query::{Expr, Query};
use sqlx::{PgPool, postgres::types::PgInterval};
use std::num::NonZeroU32;
use strum::IntoEnumIterator;

/// Seconds in one day, the unit the chunk interval is configured in.
const SECONDS_PER_DAY: i64 = 86_400;

/// Days `TimescaleDB` counts one interval month as when it stores an interval.
const DAYS_PER_MONTH: i64 = 30;

/// Whether one stored policy value was already what the configuration names.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PolicyReconciliation {
    /// The stored value already matched, or the configuration names none.
    Unchanged,
    /// The stored value was moved to the configured one.
    Adjusted,
    /// The value has no bearing on this edition: the Apache-2.0 core holds no
    /// columnstore, so it is neither read nor written. Only the columnstore
    /// and chunk skipping, whose ranges only a conversion records, ever report
    /// this; the chunk interval is core surface.
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
    /// Whether its chunk skipping columns were registered.
    pub chunk_skipping: PolicyReconciliation,
}

/// What reconciling every temporal hypertable did.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TemporalStoragePolicyOutcome {
    /// One entry per hypertable, in catalogue order.
    pub hypertables: Vec<HypertableReconciliation>,
}

impl TemporalStoragePolicyOutcome {
    /// Whether nothing was written: every stored value already matched, or the
    /// edition holds no columnstore to enable.
    #[must_use]
    pub fn is_unchanged(&self) -> bool {
        self.hypertables.iter().all(|reconciliation| {
            reconciliation.chunk_interval.wrote_nothing() && reconciliation.columnstore_enabled.wrote_nothing() && reconciliation.chunk_skipping.wrote_nothing()
        })
    }
}

impl TemporalRepository {
    /// Move every temporal hypertable's chunk interval to what `policy` names,
    /// leaving it untouched when the policy is silent on it, and, on an
    /// edition with a columnstore, enable the columnstore and register the
    /// columns chunk skipping keeps ranges of.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalStoreError::Database`] when an information view cannot
    /// be read or a policy statement fails or waits out its lock on every
    /// attempt, [`TemporalStoreError::Invariant`] when
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
            let (columnstore_enabled, chunk_skipping) = match plan_columnstore_reconciliation(self.columnstore) {
                ColumnstoreReconciliation::Unsupported => (PolicyReconciliation::Unsupported, PolicyReconciliation::Unsupported),
                ColumnstoreReconciliation::Reconcile => (
                    reconcile_columnstore_enablement(&self.pool, hypertable).await?,
                    reconcile_chunk_skipping(&self.pool, hypertable).await?,
                ),
            };
            hypertables.push(HypertableReconciliation {
                hypertable,
                chunk_interval,
                columnstore_enabled,
                chunk_skipping,
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
    /// The columnstore is to be enabled under the declared settings.
    Reconcile,
}

/// Decide what the columnstore means for one hypertable before any statement
/// runs: [`Unsupported`](ColumnstoreReconciliation::Unsupported) on the
/// Apache-2.0 core and reconciled on the TSL edition.
const fn plan_columnstore_reconciliation(support: ColumnstoreSupport) -> ColumnstoreReconciliation {
    match support {
        ColumnstoreSupport::Absent => ColumnstoreReconciliation::Unsupported,
        ColumnstoreSupport::Present => ColumnstoreReconciliation::Reconcile,
    }
}

/// Set the chunk interval of `hypertable` to `days` unless it already is.
async fn reconcile_chunk_interval(pool: &PgPool, hypertable: HypertableName, days: NonZeroU32) -> Result<PolicyReconciliation, TemporalStoreError> {
    let stored: Option<PgInterval> = chunk_interval_statement(hypertable.into()).to_query_scalar().fetch_optional(pool).await?;
    let stored = stored.ok_or(TemporalStoreInvariant::NoTimeDimension(hypertable))?;
    if interval_seconds(&stored) == day_seconds(days) {
        return Ok(PolicyReconciliation::Unchanged);
    }
    let set_interval = Query::select()
        .expr(set_chunk_time_interval(hypertable.into(), Expr::val(day_count(days))))
        .to_owned();
    execute_with_bounded_lock_wait(pool, &StatementWrite(set_interval)).await?;
    Ok(PolicyReconciliation::Adjusted)
}

/// The whole seconds an interval spans, counting a month as thirty days as
/// `TimescaleDB` does when it stores a day-denominated interval.
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
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{
        columnstore_capability::ColumnstoreSupport,
        hypertable_policy::HypertableName,
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
                chunk_skipping: PolicyReconciliation::Unchanged,
            }],
        };
        assert!(untouched.is_unchanged());
        let adjusted = TemporalStoragePolicyOutcome {
            hypertables: vec![HypertableReconciliation {
                hypertable: HypertableName::AttributeHistory,
                chunk_interval: PolicyReconciliation::Adjusted,
                columnstore_enabled: PolicyReconciliation::Unchanged,
                chunk_skipping: PolicyReconciliation::Unchanged,
            }],
        };
        assert!(!adjusted.is_unchanged());
        let registered = TemporalStoragePolicyOutcome {
            hypertables: vec![HypertableReconciliation {
                hypertable: HypertableName::AttributeHistory,
                chunk_interval: PolicyReconciliation::Unchanged,
                columnstore_enabled: PolicyReconciliation::Unchanged,
                chunk_skipping: PolicyReconciliation::Adjusted,
            }],
        };
        assert!(!registered.is_unchanged());
    }

    #[test]
    fn an_unsupported_columnstore_over_an_unchanged_chunk_interval_still_wrote_nothing() {
        let apache = TemporalStoragePolicyOutcome {
            hypertables: vec![HypertableReconciliation {
                hypertable: HypertableName::AttributeHistory,
                chunk_interval: PolicyReconciliation::Unchanged,
                columnstore_enabled: PolicyReconciliation::Unsupported,
                chunk_skipping: PolicyReconciliation::Unsupported,
            }],
        };
        assert!(apache.is_unchanged(), "the Apache core holds no columnstore to enable");
        let apache_with_adjusted_interval = TemporalStoragePolicyOutcome {
            hypertables: vec![HypertableReconciliation {
                hypertable: HypertableName::AttributeHistory,
                chunk_interval: PolicyReconciliation::Adjusted,
                columnstore_enabled: PolicyReconciliation::Unsupported,
                chunk_skipping: PolicyReconciliation::Unsupported,
            }],
        };
        assert!(!apache_with_adjusted_interval.is_unchanged(), "the Apache-safe chunk interval was still moved");
    }

    #[test]
    fn the_apache_edition_resolves_the_columnstore_to_unsupported_without_a_statement() {
        assert_eq!(
            plan_columnstore_reconciliation(ColumnstoreSupport::Absent),
            ColumnstoreReconciliation::Unsupported,
            "an Apache tenant issues no columnstore statement"
        );
    }

    #[test]
    fn the_tsl_edition_reconciles_the_columnstore() {
        assert_eq!(
            plan_columnstore_reconciliation(ColumnstoreSupport::Present),
            ColumnstoreReconciliation::Reconcile
        );
    }

    #[test]
    fn enabling_the_columnstore_is_a_write() {
        let enabled = TemporalStoragePolicyOutcome {
            hypertables: vec![HypertableReconciliation {
                hypertable: HypertableName::AttributeHistory,
                chunk_interval: PolicyReconciliation::Unchanged,
                columnstore_enabled: PolicyReconciliation::Adjusted,
                chunk_skipping: PolicyReconciliation::Unchanged,
            }],
        };
        assert!(!enabled.is_unchanged());
    }
}
