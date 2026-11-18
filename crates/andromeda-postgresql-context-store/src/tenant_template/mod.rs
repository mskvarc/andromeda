//! The tenant template: a database holding the tenant schema, marked a
//! template, that a new tenant's database is copied from.
//!
//! Copying a migrated database takes a fraction of the time migrating an empty
//! one takes. The template is a cache and never decides a schema: every copy is
//! migrated as any new tenant database is, so a copy of a template built
//! against earlier migrations is brought forward by the later ones, and a
//! broker without a template creates and migrates an empty database.
//!
//! The broker prepares the template when it starts, before it serves. It keeps
//! the template its own migrations and the server's extensions name, builds it
//! when it is missing or unfinished, and drops every other template of its
//! prefix. A template that cannot be prepared leaves the broker serving
//! without one, which is the case when the role may not stop the template's
//! `TimescaleDB` scheduler.
//!
//! A copy carries the template's schema stamp, whose instant marks one
//! generation of a tenant's schema, so every copy is stamped with an instant
//! of its own before it is migrated.

/// The schema stamp of a database copied from the tenant template.
pub(crate) mod copy_stamp;
mod template_build;
mod template_census;
mod template_name;
/// Dropping a template the broker no longer copies from.
pub mod template_retirement;

use crate::{
    database_defaults::DatabaseDefaults,
    error::ContextStoreError,
    extension_standing::extension_standings,
    schema_contract::contract::current_tenant_schema_contract,
    tenant_template::{
        template_build::build_template,
        template_census::{TemplateDatabase, TemplateReadiness, template_census},
        template_name::template_database_name,
        template_retirement::retire_and_report,
    },
};
use andromeda_configuration::schema::db_name_prefix::DbNamePrefix;
use andromeda_ngsi_ld_lexicon::tenant_database_name::TenantDatabaseName;
use andromeda_postgresql_pool::database_url::DatabaseUrl;
use sqlx::{Connection, PgConnection};
use std::time::Duration;

/// A template ready to be copied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TenantTemplate {
    /// The template's database.
    database: TenantDatabaseName,
}

impl TenantTemplate {
    /// The template's database, which a copy names as its template.
    pub(crate) const fn database(&self) -> &TenantDatabaseName {
        &self.database
    }
}

/// Make sure the server `direct_url` reaches holds the template this broker
/// copies tenant databases of `prefix` from, building it with the session
/// defaults `defaults` and the migration pool's `acquire_timeout` when it is
/// missing or unfinished, and drop every other template of `prefix`.
///
/// # Errors
///
/// Returns [`ContextStoreError::SchemaContract`] when the embedded migration
/// set has no contract or a shipped extension version cannot be read,
/// [`ContextStoreError::Database`] when the catalogue cannot be read, and
/// every failure building the template returns.
pub(crate) async fn prepare_tenant_template(
    direct_url: &DatabaseUrl,
    prefix: &DbNamePrefix,
    defaults: &DatabaseDefaults,
    acquire_timeout: Duration,
) -> Result<TenantTemplate, ContextStoreError> {
    let expected = current_template_name(direct_url, prefix).await?;
    let (current, superseded): (Vec<TemplateDatabase>, Vec<TemplateDatabase>) = template_census(direct_url, prefix)
        .await?
        .into_iter()
        .partition(|template| template.name == expected && template.readiness == TemplateReadiness::Ready);
    for template in &superseded {
        retire_and_report(direct_url, &template.name).await;
    }
    if current.is_empty() {
        build_template(direct_url, &expected, defaults, acquire_timeout).await?;
        tracing::info!(template = %expected, event = "tenant_template_built", "the tenant template was built");
    }
    Ok(TenantTemplate { database: expected })
}

/// The name of the template a broker of this build copies tenant databases of
/// `prefix` from, on the server `direct_url` reaches.
///
/// # Errors
///
/// Returns [`ContextStoreError::SchemaContract`] when the embedded migration
/// set has no contract or a shipped extension version cannot be read, and
/// [`ContextStoreError::Database`] when the connection cannot be opened or
/// closed or the version read fails.
pub async fn current_template_name(direct_url: &DatabaseUrl, prefix: &DbNamePrefix) -> Result<TenantDatabaseName, ContextStoreError> {
    let contract = current_tenant_schema_contract()?;
    let mut connection = PgConnection::connect(direct_url.as_ref().as_str()).await?;
    let standings = extension_standings(&mut connection).await;
    connection.close().await?;
    Ok(template_database_name(prefix, &contract, &standings?))
}
