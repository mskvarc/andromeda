//! How a driver error a pending-work probe hit reaches the wake loop.
//!
//! Every store that answers the wake loop for a dormant tenant reads the
//! tenant's database over a transient connection, and the wake loop reacts to
//! three kinds of failure differently: backpressure is retried, a database
//! that no longer exists clears the tenant's schedule, and anything else is a
//! fault that is retried and recorded. The split is decided here once for
//! every probe.

use crate::{database_url::names_missing_database, persistence_classification::persistence_failure, saturation::is_pool_saturated};
use andromeda_ngsi_ld_lexicon::tenant_database_name::TenantDatabaseName;
use andromeda_tenancy::pending_work::PendingWorkError;

/// The pending-work failure a probe of `database` reports for `error`.
#[track_caller]
#[must_use]
pub fn pending_work_failure(error: &sqlx::Error, database: &TenantDatabaseName) -> PendingWorkError {
    if is_pool_saturated(error) {
        PendingWorkError::Saturated {
            database: database.clone(), // clone: the error outlives this borrow
        }
    } else if names_missing_database(error) {
        PendingWorkError::DatabaseGone {
            database: database.clone(), // clone: the error outlives this borrow
        }
    } else {
        PendingWorkError::ProbeFailed {
            database: database.clone(), // clone: the error outlives this borrow
            cause: persistence_failure(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{fake_database_error::FakeDatabaseError, pending_work_failure::pending_work_failure};
    use andromeda_ngsi_ld_lexicon::tenant_database_name::TenantDatabaseName;
    use andromeda_tenancy::pending_work::PendingWorkError;

    #[test]
    fn a_probe_failure_is_saturation_a_gone_database_or_a_fault() {
        let database = TenantDatabaseName::from("andromeda_tenant_gone".to_owned());
        assert!(matches!(
            pending_work_failure(&FakeDatabaseError::new("no database", "3D000").into_sqlx(), &database),
            PendingWorkError::DatabaseGone { .. }
        ));
        assert!(matches!(
            pending_work_failure(&sqlx::Error::PoolTimedOut, &database),
            PendingWorkError::Saturated { .. }
        ));
        assert!(matches!(
            pending_work_failure(&FakeDatabaseError::new("no table", "42P01").into_sqlx(), &database),
            PendingWorkError::ProbeFailed { .. }
        ));
        assert!(matches!(
            pending_work_failure(&sqlx::Error::RowNotFound, &database),
            PendingWorkError::ProbeFailed { .. }
        ));
    }
}
