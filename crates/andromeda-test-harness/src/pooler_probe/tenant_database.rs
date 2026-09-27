//! Reaching one tenant's own database on the transaction-pooling tier, over the
//! direct endpoint rather than through the pooler.
//!
//! The database is located by the tenant it serves, as the platform registry
//! names it, never by guessing among the tenant databases the server holds: a
//! scenario that provisions more than one tenant, or runs after one that left a
//! database behind, must still reach the right one.

use crate::{
    configuration::{ConfigurationError, DatabaseEndpoint},
    pooler_probe::endpoints,
    tenant_provisioning::registered_tenant_database,
};
use andromeda_ngsi_ld_lexicon::tenant::TenantName;
use andromeda_tenancy::database_name::TenantDatabaseName;
use sqlx::PgPool;
use thiserror::Error;

/// Failure to locate or reach a tenant's database.
#[derive(Debug, Error)]
pub enum TenantDatabaseError {
    /// The platform registry could not be read.
    #[error("the platform tenant registry could not be read: {0}")]
    Registry(#[source] sqlx::Error),
    /// The platform registry names no database for the tenant.
    #[error("the platform registry names no database for tenant {tenant}")]
    Unregistered {
        /// The tenant the registry was asked about.
        tenant: TenantName,
    },
    /// The direct endpoint the tier is configured with is not a PostgreSQL URL.
    #[error("the direct database endpoint is invalid: {0}")]
    Endpoint(#[from] ConfigurationError),
    /// The tenant database refused the connection.
    #[error("the tenant database could not be reached: {0}")]
    Connect(#[source] sqlx::Error),
}

/// The name of the database the platform registry behind `control` holds
/// `tenant` in.
///
/// # Errors
///
/// Returns [`TenantDatabaseError::Registry`] when the registry cannot be read,
/// and [`TenantDatabaseError::Unregistered`] when it names no database for
/// `tenant`.
pub async fn tenant_database_name(control: &PgPool, tenant: &TenantName) -> Result<TenantDatabaseName, TenantDatabaseError> {
    registered_tenant_database(control, tenant)
        .await
        .map_err(TenantDatabaseError::Registry)?
        .ok_or_else(|| TenantDatabaseError::Unregistered {
            tenant: tenant.clone(), // clone: the error owns the tenant it reports.
        })
}

/// A direct connection to `database` on the tier's PostgreSQL server, with the
/// credentials of the direct endpoint.
///
/// # Errors
///
/// Returns [`TenantDatabaseError::Endpoint`] when the direct endpoint is not a
/// PostgreSQL URL, and [`TenantDatabaseError::Connect`] when the database
/// refuses the connection.
pub async fn connect_database(database: &TenantDatabaseName) -> Result<PgPool, TenantDatabaseError> {
    let endpoint = DatabaseEndpoint::parse(endpoints::database_url())?.with_database(database.as_str());
    PgPool::connect(endpoint.url().as_str()).await.map_err(TenantDatabaseError::Connect)
}

/// A direct connection to the database the platform registry behind `control`
/// holds `tenant` in.
///
/// # Errors
///
/// Returns every failure of [`tenant_database_name`] and [`connect_database`].
pub async fn connect_tenant_database(control: &PgPool, tenant: &TenantName) -> Result<PgPool, TenantDatabaseError> {
    connect_database(&tenant_database_name(control, tenant).await?).await
}
