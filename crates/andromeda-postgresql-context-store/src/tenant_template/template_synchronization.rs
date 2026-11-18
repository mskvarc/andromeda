//! Bringing the tenant template to this build's schema and locking it again.
//!
//! The template is unlocked, migrated as a tenant database is when it opens,
//! then locked. It is locked before its `TimescaleDB` scheduler is stopped, by
//! a session opened while it still admitted connections: once it refuses
//! connections no scheduler attaches to it again, a server restart included,
//! so none can attach between the stop and the lock. A start cut short while
//! the template is unlocked leaves it unlocked, and the next start locks it.

use crate::{
    database_defaults::DatabaseDefaults,
    error::ContextStoreError,
    tenant_database::run_database_definition,
    tenant_migration::migrate_single_tenant_db,
};
use andromeda_ngsi_ld_lexicon::tenant_database_name::TenantDatabaseName;
use andromeda_postgresql_pool::{database_url::DatabaseUrl, schema_change_lock::schema_change_connect_options};
use andromeda_postgresql_seaquery::{database_definition::DatabaseDefinition, statement::Statement, timescale_scheduler::stop_background_workers};
use sea_query::Alias;
use sqlx::{Connection, PgConnection};
use std::time::Duration;
use url::Url;

/// Unlock `template` on the server `direct_url` reaches, bring it to this
/// build's schema with the session defaults `defaults` and the migration
/// pool's `acquire_timeout`, and lock it again with its scheduler stopped.
///
/// # Errors
///
/// Returns the errors [`migrate_single_tenant_db`] returns, among them
/// [`ContextStoreError::Migration`] and [`ContextStoreError::SchemaContract`]
/// for a schema this build cannot bring forward, and
/// [`ContextStoreError::Database`] when the template cannot be unlocked or
/// locked or its scheduler cannot be stopped, the role lacking the privilege
/// included.
pub(crate) async fn synchronize_template(
    direct_url: &DatabaseUrl,
    template: &TenantDatabaseName,
    defaults: &DatabaseDefaults,
    acquire_timeout: Duration,
) -> Result<(), ContextStoreError> {
    run_database_definition(direct_url, &DatabaseDefinition::unlock_template(Alias::new(template.as_str()))).await?;
    let template_url = direct_url.with_database_name(template);
    migrate_single_tenant_db(&template_url, defaults, acquire_timeout).await?;
    lock_with_scheduler_stopped(direct_url, template, &template_url).await
}

/// Lock `template`, then stop its scheduler over a session opened on
/// `template_url` before the lock.
async fn lock_with_scheduler_stopped(direct_url: &DatabaseUrl, template: &TenantDatabaseName, template_url: &Url) -> Result<(), ContextStoreError> {
    let options = schema_change_connect_options(template_url.as_str())?;
    let mut inside = PgConnection::connect_with(&options).await?;
    let stopped = match run_database_definition(direct_url, &DatabaseDefinition::lock_template(Alias::new(template.as_str()))).await {
        Ok(()) => stop_background_workers().to_query().execute(&mut inside).await.map(drop),
        Err(error) => Err(error),
    };
    inside.close().await?;
    Ok(stopped?)
}
