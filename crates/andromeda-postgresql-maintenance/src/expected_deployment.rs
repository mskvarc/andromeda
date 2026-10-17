//! The facts of the broker's own configuration the controller checks the
//! cluster against, which no catalogue can tell it.

use andromeda_tenancy::db_name_prefix::DbNamePrefix;
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use std::time::Duration;

/// How the broker converts temporal history to the columnstore.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ColumnstoreConversion {
    /// `temporal.columnstore_after`: a chunk is due once its range ended this
    /// long ago.
    pub after: Duration,
    /// `temporal.columnstore_sweep_interval`: how often the sweep of each open
    /// tenant runs.
    pub sweep_interval: Duration,
}

impl ColumnstoreConversion {
    /// The latest range end a chunk may still be in the rowstore with at `now`
    /// without being overdue, `None` when the configured spans reach past the
    /// representable instants and no chunk can be overdue.
    ///
    /// A chunk falls due `after` its range ends and the next sweep converts it;
    /// a sweep that deferred it on a lock wait tries again one interval later.
    /// Past `after` plus two sweep intervals the sweep has missed it.
    pub(crate) fn overdue_cutoff(self, now: DateTime<Utc>) -> Option<DateTime<Utc>> {
        let after = ChronoDuration::from_std(self.after).ok()?;
        let sweeps = ChronoDuration::from_std(self.sweep_interval).ok()?.checked_mul(2)?;
        now.checked_sub_signed(after.checked_add(&sweeps)?)
    }
}

/// What the controller expects of the deployment it watches.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExpectedDeployment {
    /// The prefix every tenant database name starts with, which tells a tenant
    /// database left without a registry row from the server's own databases.
    pub tenant_database_prefix: DbNamePrefix,
    /// How history is converted to the columnstore.
    pub columnstore_conversion: ColumnstoreConversion,
}

#[cfg(test)]
mod tests {
    use crate::expected_deployment::ColumnstoreConversion;
    use chrono::{Duration as ChronoDuration, TimeZone, Utc};
    use pretty_assertions::assert_eq;
    use std::time::Duration;

    #[test]
    fn a_chunk_is_overdue_past_the_horizon_and_two_sweep_intervals() {
        let conversion = ColumnstoreConversion {
            after: Duration::from_hours(30 * 24),
            sweep_interval: Duration::from_hours(1),
        };
        let now = Utc.with_ymd_and_hms(2026, 10, 17, 12, 0, 0).single().expect("a valid instant");
        assert_eq!(conversion.overdue_cutoff(now), Some(now - ChronoDuration::days(30) - ChronoDuration::hours(2)));
    }

    #[test]
    fn spans_past_the_representable_instants_leave_no_chunk_overdue() {
        let conversion = ColumnstoreConversion {
            after: Duration::from_secs(u64::MAX),
            sweep_interval: Duration::from_secs(3_600),
        };
        assert_eq!(conversion.overdue_cutoff(Utc::now()), None);
    }
}
