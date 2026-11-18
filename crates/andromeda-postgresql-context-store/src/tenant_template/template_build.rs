//! Building the tenant template: an empty database migrated as a new tenant's
//! is, its `TimescaleDB` scheduler stopped, then marked a template that
//! refuses connections.
//!
//! The scheduler is stopped before the mark because a template that still
//! admits connections has its scheduler restarted on every copy, and the
//! restarted scheduler's session fails the copy. Once the template refuses
//! connections the extension's launcher never schedules it again, a server
//! restart included.

use crate::{
    database_defaults::DatabaseDefaults,
    error::ContextStoreError,
    tenant_database::{DatabaseSource, create_database_if_missing},
    tenant_migration::migrate_single_tenant_db,
    tenant_template::template_retirement::retire_and_report,
};
use andromeda_ngsi_ld_lexicon::tenant_database_name::TenantDatabaseName;
use andromeda_postgresql_pool::{database_url::DatabaseUrl, schema_change_lock::schema_change_connect_options};
use andromeda_postgresql_seaquery::{
    command::Command,
    database_definition::DatabaseDefinition,
    statement::Statement,
    timescale_scheduler::stop_background_workers,
};
use sea_query::Alias;
use sqlx::{Connection, PgConnection};
use std::time::Duration;
use url::Url;

/// Build the template `template` on the server `direct_url` reaches, migrated
/// with the session defaults `defaults` and the migration pool's
/// `acquire_timeout`.
///
/// A database of that name left by a build cut short is built on. A build that
/// fails drops what it made.
///
/// # Errors
///
/// Returns the errors [`create_database_if_missing`] and
/// [`migrate_single_tenant_db`] return, and [`ContextStoreError::Database`]
/// when the scheduler cannot be stopped, the role lacking the privilege
/// included, or the template cannot be marked.
pub(crate) async fn build_template(
    direct_url: &DatabaseUrl,
    template: &TenantDatabaseName,
    defaults: &DatabaseDefaults,
    acquire_timeout: Duration,
) -> Result<(), ContextStoreError> {
    create_database_if_missing(direct_url, template, DatabaseSource::ServerDefault).await?;
    let built = migrate_and_mark(direct_url, template, defaults, acquire_timeout).await;
    if built.is_err() {
        retire_and_report(direct_url, template).await;
    }
    built
}

/// Migrate the created template, stop its scheduler and mark it.
async fn migrate_and_mark(
    direct_url: &DatabaseUrl,
    template: &TenantDatabaseName,
    defaults: &DatabaseDefaults,
    acquire_timeout: Duration,
) -> Result<(), ContextStoreError> {
    let template_url = direct_url.with_database_name(template);
    migrate_single_tenant_db(&template_url, defaults, acquire_timeout).await?;
    stop_scheduler(&template_url).await?;
    mark_template(direct_url, template).await
}

/// Stop the `TimescaleDB` scheduler of the database `database_url` names, over
/// a connection of its own.
async fn stop_scheduler(database_url: &Url) -> Result<(), ContextStoreError> {
    let options = schema_change_connect_options(database_url.as_str())?;
    let mut connection = PgConnection::connect_with(&options).await?;
    let stopped = stop_background_workers().to_query().execute(&mut connection).await;
    connection.close().await?;
    stopped?;
    Ok(())
}

/// Mark `template` a template that refuses connections, from a connection to
/// the database `direct_url` names.
async fn mark_template(direct_url: &DatabaseUrl, template: &TenantDatabaseName) -> Result<(), ContextStoreError> {
    let mut connection = PgConnection::connect(direct_url.as_ref().as_str()).await?;
    let marked = DatabaseDefinition::mark_template(Alias::new(template.as_str()))
        .to_query()
        .execute(&mut connection)
        .await;
    connection.close().await?;
    marked?;
    Ok(())
}
