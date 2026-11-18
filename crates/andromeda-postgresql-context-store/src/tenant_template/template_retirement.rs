//! Dropping the tenant template, to build it again or to remove it.

use crate::{
    error::ContextStoreError,
    tenant_database::{DatabaseDrop, drop_database_unless_in_use, run_database_definition},
};
use andromeda_ngsi_ld_lexicon::tenant_database_name::TenantDatabaseName;
use andromeda_postgresql_pool::database_url::{DatabaseUrl, names_missing_database};
use andromeda_postgresql_seaquery::database_definition::DatabaseDefinition;
use sea_query::Alias;

/// Drop the template database `template`, from a connection to the database
/// `direct_url` names, unlocking it first because PostgreSQL refuses to drop
/// a template.
///
/// A template that no longer exists counts as dropped. One a session still
/// holds is left in place, unlocked.
///
/// # Errors
///
/// Returns [`ContextStoreError::Database`] when a connection cannot be opened
/// or closed, or the unlocking or the drop fails for a reason other than a
/// connected session or a held lock.
pub async fn retire_template(direct_url: &DatabaseUrl, template: &TenantDatabaseName) -> Result<DatabaseDrop, ContextStoreError> {
    match run_database_definition(direct_url, &DatabaseDefinition::unlock_template(Alias::new(template.as_str()))).await {
        Ok(()) => drop_database_unless_in_use(direct_url, template).await,
        Err(error) if names_missing_database(&error) => Ok(DatabaseDrop::Dropped),
        Err(error) => Err(error.into()),
    }
}
