//! Platform-database coordination and durable evidence.
//!
//! The controller's state lives in the platform database: this module owns the
//! fleet-wide lock and the identity of a registered tenant,
//! [`autovacuum_saturation`] owns the cluster's run of passes with every
//! autovacuum worker busy, [`tenant_inspection`] owns what is recorded per tenant,
//! [`target_record`] what is recorded per target, [`pending_action`] which
//! queued action a visit runs next, [`open_breach`] which breach runs triage
//! waits on, [`branch_rows`] which context branch a
//! target row belongs to, [`gin_key_layout`] the keys per row and bytes per key
//! a density-measured GIN is judged by and [`analyzed_rows`] the row count a
//! heap or a density-measured GIN is judged by, and [`repack_run`] which
//! repacks have objects in a tenant database.

/// The analyzed row count stored on each heap and density-measured GIN.
pub(crate) mod analyzed_rows;
/// The cluster's run of passes that found every autovacuum worker busy.
pub(crate) mod autovacuum_saturation;
/// The context branch dimension of the target rows.
pub(crate) mod branch_rows;
/// The keys per row and bytes per key stored on each density-measured GIN.
pub(crate) mod gin_key_layout;
/// The open breach runs that triage waits on.
pub(crate) mod open_breach;
/// Which queued action a visit runs next.
pub(crate) mod pending_action;
/// The durable record of every repack whose objects are in a tenant
/// database.
pub(crate) mod repack_run;
/// The durable decision and action state of one maintenance target.
pub(crate) mod target_record;
/// The durable evidence of one tenant's inspection.
pub(crate) mod tenant_inspection;

use crate::error::MaintenanceError;
use andromeda_ngsi_ld_lexicon::{tenant::TenantName, tenant_database_name::TenantDatabaseName};
use andromeda_postgresql_seaquery::{
    advisory_lock::{advisory_unlock, try_advisory_lock},
    statement::Statement,
};
use sea_query::{Expr, Query};
use sqlx::PgConnection;

/// One of the fleet-wide session advisory locks the controller coordinates
/// its replicas by.
#[derive(Clone, Copy, Debug, Eq, PartialEq, strum::Display)]
#[strum(serialize_all = "kebab-case")]
pub enum FleetLock {
    /// Held by the one pass running anywhere in the fleet.
    Pass,
    /// Held by the one repack running anywhere in the fleet, beside the pass,
    /// which keeps visiting every tenant while a repack runs.
    Repack,
}

impl FleetLock {
    /// The lock's stable key.
    const fn key(self) -> &'static str {
        match self {
            FleetLock::Pass => "andromeda-postgresql-maintenance",
            FleetLock::Repack => "andromeda-postgresql-maintenance-repack",
        }
    }
}

/// Stable primary key of one platform tenant row.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(crate) struct TenantKey(pub(crate) i64);

/// One registered database selected independently of the runtime cache.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RegisteredTenant {
    /// Platform row identity used by durable state.
    pub(crate) key: TenantKey,
    /// Absent only for the unnamed default tenant.
    pub(crate) name: Option<TenantName>,
    /// PostgreSQL database name.
    pub(crate) database: TenantDatabaseName,
}

impl RegisteredTenant {
    /// Bounded log label that never becomes a metric dimension.
    pub(crate) fn log_name(&self) -> &str {
        self.name.as_ref().map_or("<default>", TenantName::as_str)
    }
}

/// Whether a session took a fleet lock.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum FleetLockTaken {
    /// This session holds the lock.
    Taken,
    /// Another broker's session holds it.
    HeldElsewhere,
}

/// Try to obtain `lock` for the session without waiting.
pub(crate) async fn try_fleet_lock(connection: &mut PgConnection, lock: FleetLock) -> Result<FleetLockTaken, MaintenanceError> {
    let statement = Query::select().expr(try_advisory_lock(Expr::val(lock.key()))).to_owned();
    Ok(if statement.to_query_scalar::<bool>().fetch_one(connection).await? {
        FleetLockTaken::Taken
    } else {
        FleetLockTaken::HeldElsewhere
    })
}

/// Release `lock`, which this session holds.
pub(crate) async fn release_fleet_lock(connection: &mut PgConnection, lock: FleetLock) -> Result<(), MaintenanceError> {
    let statement = Query::select().expr(advisory_unlock(Expr::val(lock.key()))).to_owned();
    let released = statement.to_query_scalar::<bool>().fetch_one(connection).await?;
    if released { Ok(()) } else { Err(MaintenanceError::FleetLockNotHeld(lock)) }
}

/// Whether another broker holds `lock`, read by taking it and releasing it
/// at once.
pub(crate) async fn fleet_lock_held_elsewhere(connection: &mut PgConnection, lock: FleetLock) -> Result<FleetLockTaken, MaintenanceError> {
    let taken = try_fleet_lock(&mut *connection, lock).await?;
    if taken == FleetLockTaken::Taken {
        release_fleet_lock(connection, lock).await?;
    }
    Ok(taken)
}

#[cfg(test)]
mod tests {
    use crate::state::{FleetLock, RegisteredTenant, TenantKey};
    use andromeda_ngsi_ld_lexicon::tenant_database_name::TenantDatabaseName;
    use pretty_assertions::assert_eq;

    #[test]
    fn unnamed_default_has_an_explicit_log_label() {
        let tenant = RegisteredTenant {
            key: TenantKey(1),
            name: None,
            database: TenantDatabaseName::from("postgres".to_owned()),
        };
        assert_eq!(tenant.log_name(), "<default>");
    }

    #[test]
    fn the_pass_and_the_repack_take_different_fleet_locks() {
        assert_eq!(FleetLock::Pass.key(), "andromeda-postgresql-maintenance");
        assert_eq!(FleetLock::Repack.key(), "andromeda-postgresql-maintenance-repack");
    }
}
