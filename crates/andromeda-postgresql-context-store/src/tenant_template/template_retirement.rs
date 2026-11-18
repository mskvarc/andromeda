//! Dropping a template the broker no longer copies from.

use crate::{
    error::ContextStoreError,
    tenant_database::{DatabaseDrop, drop_database_unless_in_use},
};
use andromeda_ngsi_ld_lexicon::tenant_database_name::TenantDatabaseName;
use andromeda_postgresql_pool::database_url::{DatabaseUrl, names_missing_database};
use andromeda_postgresql_seaquery::{command::Command, database_definition::DatabaseDefinition};
use sea_query::Alias;
use sqlx::{Connection, PgConnection};

/// Drop the template database `template`, from a connection to the database
/// `direct_url` names, unmarking it first because PostgreSQL refuses to drop
/// a template.
///
/// A template that no longer exists counts as dropped. One a session still
/// holds is left in place, unmarked.
///
/// # Errors
///
/// Returns [`ContextStoreError::Database`] when a connection cannot be opened
/// or closed, or the unmarking or the drop fails for a reason other than a
/// connected session.
pub async fn retire_template(direct_url: &DatabaseUrl, template: &TenantDatabaseName) -> Result<DatabaseDrop, ContextStoreError> {
    let mut connection = PgConnection::connect(direct_url.as_ref().as_str()).await?;
    let unmarked = DatabaseDefinition::unmark_template(Alias::new(template.as_str()))
        .to_query()
        .execute(&mut connection)
        .await;
    connection.close().await?;
    match unmarked {
        Ok(_) => drop_database_unless_in_use(direct_url, template).await,
        Err(error) if names_missing_database(&error) => Ok(DatabaseDrop::Dropped),
        Err(error) => Err(error.into()),
    }
}

/// Retire `template`, logging what keeps it in place rather than returning
/// it: a template left behind costs disk and nothing else, and the next start
/// tries again.
pub(crate) async fn retire_and_report(direct_url: &DatabaseUrl, template: &TenantDatabaseName) {
    match retire_template(direct_url, template).await {
        Ok(DatabaseDrop::Dropped) => {
            tracing::info!(template = %template, event = "tenant_template_dropped", "a tenant template the broker no longer copies from was dropped");
        }
        Ok(DatabaseDrop::InUse) => {
            tracing::warn!(
                template = %template,
                event = "tenant_template_kept",
                "a tenant template the broker no longer copies from is still connected to and was kept"
            );
        }
        Err(error) => {
            tracing::warn!(
                template = %template,
                %error,
                event = "tenant_template_kept",
                "a tenant template the broker no longer copies from could not be dropped"
            );
        }
    }
}
