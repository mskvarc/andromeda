//! What an observation-only check reports.
//!
//! Every check that only watches (no action follows from it) reports a finding:
//! a stable decision name an operator can alert on, and the level the event is
//! logged at. A check maps its catalogue rows to findings with a pure function,
//! so the mapping is testable without a server.

use strum::{Display, IntoStaticStr};

/// The stable decision name a finding is logged with.
#[derive(Clone, Copy, Debug, Display, Eq, IntoStaticStr, PartialEq)]
#[strum(serialize_all = "kebab-case")]
pub(crate) enum ObservationDecision {
    /// A database the controller does not visit, the server's own or one no
    /// active tenant owns, is behind on anti-wraparound vacuum.
    FreezePressure,
    /// A database named like a tenant database has no registry row.
    OrphanTenantDatabase,
    /// A history chunk past the columnstore horizon is still in the rowstore.
    ColumnstoreOverdue,
    /// A history chunk past the retention age is still stored.
    RetentionOverdue,
    /// A `TimescaleDB` job exists that the broker did not create.
    TimescaledbJobUnexpected,
    /// A replication slot keeps WAL beyond `max_wal_size`.
    ReplicationSlotRetainingWal,
    /// A replication slot's consumer needs WAL the server has removed.
    ReplicationSlotLost,
    /// Autovacuum has not vacuumed a heap past twice its trigger.
    AutovacuumOverdue,
    /// Autoanalyze has missed a heap the controller observes but never
    /// analyzes itself.
    StatisticsStale,
    /// A versioned extension is installed below the version the server ships.
    ExtensionUpdateAvailable,
    /// `pg_stat_statements` has evicted entries since its statistics were
    /// reset.
    StatementStatisticsEvicted,
}

/// The level a finding is logged at.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) enum ObservationLevel {
    /// Needs an operator's attention before it becomes an outage.
    Warning,
    /// An outage is imminent or under way.
    Error,
}

/// One finding of an observation-only check.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Finding {
    /// The decision name.
    pub(crate) decision: ObservationDecision,
    /// The level it is logged at.
    pub(crate) level: ObservationLevel,
}

impl Finding {
    /// A finding logged as a warning.
    pub(crate) const fn warning(decision: ObservationDecision) -> Finding {
        Finding {
            decision,
            level: ObservationLevel::Warning,
        }
    }

    /// A finding logged as an error.
    pub(crate) const fn error(decision: ObservationDecision) -> Finding {
        Finding {
            decision,
            level: ObservationLevel::Error,
        }
    }

    /// The decision name as it is logged.
    pub(crate) fn decision_name(self) -> &'static str {
        self.decision.into()
    }
}

#[cfg(test)]
mod tests {
    use crate::observation::{Finding, ObservationDecision, ObservationLevel};
    use pretty_assertions::assert_eq;

    #[test]
    fn every_decision_name_is_kebab_case() {
        for (decision, name) in [
            (ObservationDecision::FreezePressure, "freeze-pressure"),
            (ObservationDecision::OrphanTenantDatabase, "orphan-tenant-database"),
            (ObservationDecision::ColumnstoreOverdue, "columnstore-overdue"),
            (ObservationDecision::RetentionOverdue, "retention-overdue"),
            (ObservationDecision::TimescaledbJobUnexpected, "timescaledb-job-unexpected"),
            (ObservationDecision::ReplicationSlotRetainingWal, "replication-slot-retaining-wal"),
            (ObservationDecision::ReplicationSlotLost, "replication-slot-lost"),
            (ObservationDecision::AutovacuumOverdue, "autovacuum-overdue"),
            (ObservationDecision::StatisticsStale, "statistics-stale"),
            (ObservationDecision::ExtensionUpdateAvailable, "extension-update-available"),
            (ObservationDecision::StatementStatisticsEvicted, "statement-statistics-evicted"),
        ] {
            assert_eq!(Finding::warning(decision).decision_name(), name);
        }
    }

    #[test]
    fn an_error_outranks_a_warning() {
        assert!(ObservationLevel::Error > ObservationLevel::Warning);
        assert_eq!(Finding::error(ObservationDecision::ReplicationSlotLost).level, ObservationLevel::Error);
    }
}
