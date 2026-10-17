//! Whether a tenant's versioned extensions lag the server's installation.
//!
//! After an image upgrade every tenant database keeps the `TimescaleDB` and
//! `PostGIS` versions it last updated to until `ALTER EXTENSION ... UPDATE`
//! runs in it, which the broker does each time it opens the tenant. A tenant
//! no broker has opened since then still runs the old versions, and the
//! controller, which visits idle tenants too, is what reports it. Updating is
//! a schema change, so the controller never does it.

use crate::{
    error::MaintenanceError,
    observation::{Finding, ObservationDecision, ObservationLevel},
    state::RegisteredTenant,
};
use andromeda_postgresql_context_store::extension_update::{begin_without_timescaledb, pending_extension_updates};
use sqlx::PgConnection;

/// The finding every pending extension update reports.
pub(crate) const EXTENSION_UPDATE: Finding = Finding::warning(ObservationDecision::ExtensionUpdateAvailable);

/// Report every versioned extension of the tenant's database installed below
/// the version the server ships.
///
/// The versions are read in a transaction that keeps `TimescaleDB` unloaded,
/// as every read deciding an update does.
pub(crate) async fn report_extension_updates(connection: &mut PgConnection, tenant: &RegisteredTenant) -> Result<(), MaintenanceError> {
    let mut transaction = begin_without_timescaledb(&mut *connection).await?;
    let pending = pending_extension_updates(&mut transaction).await.map_err(MaintenanceError::ExtensionVersions)?;
    transaction.commit().await?;
    for update in &pending {
        observation_event!(EXTENSION_UPDATE, tenant = tenant.log_name(), database = %tenant.database, extension = %update.extension, installed = %update.installed, shipped = %update.shipped, "PostgreSQL extension is installed below the version the server ships; the broker updates it the next time it opens the tenant");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::{extension_observation::EXTENSION_UPDATE, observation::ObservationLevel};
    use pretty_assertions::assert_eq;

    #[test]
    fn an_extension_below_the_shipped_version_reports_extension_update_available_as_a_warning() {
        assert_eq!(EXTENSION_UPDATE.decision_name(), "extension-update-available");
        assert_eq!(EXTENSION_UPDATE.level, ObservationLevel::Warning);
    }
}
