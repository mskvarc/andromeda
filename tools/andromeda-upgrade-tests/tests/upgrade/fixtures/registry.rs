//! The broker's tenant registry opened over a rehearsal's platform database,
//! the tenant rows v1.0.0 wrote into it, and the tenant template the registry
//! prepares on the server.

use crate::fixtures::{
    statements::{insert, smallint},
    test_error::UpgradeTestError,
};
use andromeda_configuration::schema::{database::SynchronousCommit, db_name_prefix::DbNamePrefix, tenancy::TenantDatabasesConfig};
use andromeda_ngsi_ld_lexicon::tenant_database_name::TenantDatabaseName;
use andromeda_postgresql_context_store::{
    database_defaults::{DatabaseDefaults, StatementTimeout},
    tenant_database::DatabaseDrop,
    tenant_migration::open_schema_change_pool,
    tenant_registry::PostgresTenantRegistry,
    tenant_template::{current_template_name, template_retirement::retire_template},
};
use andromeda_postgresql_pool::pool_options::configure_pool_options;
use andromeda_upgrade_tests::{upgrade_database::UpgradeDatabase, upgrade_server::UpgradeServer};
use sea_query::Expr;
use sqlx::PgPool;
use std::time::Duration;
use uuid::Uuid;

/// The stored status of an active tenant.
pub(crate) const ACTIVE: i16 = 1;

/// The stored status of a tenant whose provisioning has not finished.
pub(crate) const PROVISIONAL: i16 = 3;

/// How long a registry step waits for a connection.
const ACQUIRE_TIMEOUT: Duration = Duration::from_secs(30);

/// The request deadline the registry's databases are given their statement
/// timeout from.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// A prefix no other rehearsal names its tenant databases and its template
/// after.
pub(crate) fn fresh_prefix() -> Result<DbNamePrefix, UpgradeTestError> {
    Ok(DbNamePrefix::try_from(format!("upgrade_{}", Uuid::new_v4().simple()))?)
}

/// Writes the registry row of the tenant `tenant_name` as v1.0.0 wrote it.
pub(crate) async fn insert_v1_0_0_tenant(pool: &PgPool, tenant_name: &str, database_name: &str, status: i16) -> Result<(), UpgradeTestError> {
    insert(
        pool,
        "tenants",
        vec![
            ("name", Expr::val(tenant_name)),
            ("database_name", Expr::val(database_name)),
            ("status", smallint(status)),
        ],
    )
    .await
}

/// The registry on `database`, opened as the broker opens it when it starts,
/// which brings the platform database through the migrations this build
/// embeds and prepares the tenant template, naming new tenant databases and
/// the template after `prefix`.
pub(crate) async fn open_registry(database: &UpgradeDatabase, prefix: &DbNamePrefix) -> Result<PostgresTenantRegistry, UpgradeTestError> {
    let databases = TenantDatabasesConfig {
        name_prefix: prefix.clone(),
        ..TenantDatabasesConfig::default()
    };
    let defaults = DatabaseDefaults {
        synchronous_commit: SynchronousCommit::default(),
        statement_timeout: StatementTimeout::past_request_deadline(REQUEST_TIMEOUT),
    };
    Ok(PostgresTenantRegistry::connect(
        configure_pool_options(2, ACQUIRE_TIMEOUT),
        database.database_url(),
        databases,
        &defaults,
        ACQUIRE_TIMEOUT,
    )
    .await?)
}

/// A one-connection pool on the tenant database `database` of `server`.
pub(crate) async fn open_tenant_pool(server: &UpgradeServer, database: &TenantDatabaseName) -> Result<PgPool, UpgradeTestError> {
    Ok(open_schema_change_pool(&server.url().with_database_name(database), ACQUIRE_TIMEOUT).await?)
}

/// Drops the template a registry of `prefix` prepares on `server`.
pub(crate) async fn drop_registry_template(server: &UpgradeServer, prefix: &DbNamePrefix) -> Result<(), UpgradeTestError> {
    let template = current_template_name(server.url(), prefix).await?;
    match retire_template(server.url(), &template).await? {
        DatabaseDrop::Dropped => Ok(()),
        DatabaseDrop::InUse => Err(UpgradeTestError::TemplateHeld),
    }
}
