//! Platform-database coordination and durable evidence.
//!
//! The controller's state lives in the platform database: this module owns the
//! fleet-wide lock and the identity of a registered tenant,
//! [`autovacuum_saturation`] owns the cluster's run of passes with every
//! autovacuum worker busy, [`tenant_inspection`] owns what is recorded per tenant,
//! [`target_record`] what is recorded per target, [`pending_action`] which
//! queued action a visit runs next, [`open_breach`] which breach runs triage
//! waits on, [`branch_rows`] which context branch a
//! target row belongs to and [`density_baseline`] the baseline a
//! density-measured GIN is compared with.

/// The cluster's run of passes that found every autovacuum worker busy.
pub(crate) mod autovacuum_saturation;
/// The context branch dimension of the target rows.
pub(crate) mod branch_rows;
/// The recorded density baseline of each density-measured GIN.
pub(crate) mod density_baseline;
/// The open breach runs that triage waits on.
pub(crate) mod open_breach;
/// Which queued action a visit runs next.
pub(crate) mod pending_action;
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

/// Stable key for the process-wide lock.
const GLOBAL_LOCK_KEY: &str = "andromeda-postgresql-maintenance";

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

/// Try to obtain the one fleet-wide session advisory lock without waiting.
pub(crate) async fn try_global_lock(connection: &mut PgConnection) -> Result<bool, MaintenanceError> {
    let statement = Query::select().expr(try_advisory_lock(Expr::val(GLOBAL_LOCK_KEY))).to_owned();
    Ok(statement.to_query_scalar::<bool>().fetch_one(connection).await?)
}

/// Release the fleet-wide session advisory lock.
pub(crate) async fn release_global_lock(connection: &mut PgConnection) -> Result<(), MaintenanceError> {
    let statement = Query::select().expr(advisory_unlock(Expr::val(GLOBAL_LOCK_KEY))).to_owned();
    let released = statement.to_query_scalar::<bool>().fetch_one(connection).await?;
    if released {
        Ok(())
    } else {
        Err(MaintenanceError::Database(sqlx::Error::Protocol(
            "the PostgreSQL maintenance advisory lock was not held".to_owned(),
        )))
    }
}

#[cfg(test)]
mod tests {
    use crate::state::{RegisteredTenant, TenantKey};
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
}
