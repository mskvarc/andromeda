//! The tenant template: a database holding the tenant schema, locked as a
//! template, that a new tenant's database is copied from.
//!
//! Copying a migrated database takes a fraction of the time migrating an empty
//! one takes. The template is a cache and never decides a schema: every copy is
//! migrated as any new tenant database is, so a copy of a template behind this
//! build's migrations is brought forward by them, and a broker without a
//! template creates and migrates an empty database.
//!
//! Each broker keeps one template, named after its prefix, and brings it to its
//! own schema every time it starts, before it serves, as it brings a tenant
//! database forward when the tenant opens. The broker builds the template when
//! it is missing, and builds it again when its schema is one this build cannot
//! bring forward: a stamp ahead of the build, another migration set at the same
//! version, or a migration that fails. A template that cannot be prepared
//! leaves the broker serving without one, which is the case when the role may
//! not stop the template's `TimescaleDB` scheduler.
//!
//! A copy carries the template's schema stamp, whose instant marks one
//! generation of a tenant's schema, so every copy is stamped with an instant
//! of its own before it is migrated.

/// The schema stamp of a database copied from the tenant template.
pub(crate) mod copy_stamp;
/// The name of the tenant template.
pub mod template_name;
/// Dropping the tenant template, to build it again or to remove it.
pub mod template_retirement;
mod template_synchronization;

use crate::{
    database_defaults::DatabaseDefaults,
    error::ContextStoreError,
    tenant_database::{DatabaseCreation, DatabaseDrop, DatabaseSource, create_database_if_missing},
    tenant_template::{template_name::template_database_name, template_retirement::retire_template, template_synchronization::synchronize_template},
};
use andromeda_configuration::schema::db_name_prefix::DbNamePrefix;
use andromeda_ngsi_ld_lexicon::tenant_database_name::TenantDatabaseName;
use andromeda_postgresql_pool::database_url::DatabaseUrl;
use std::time::Duration;
use thiserror::Error;

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

/// Why the tenant template could not be prepared.
#[derive(Debug, Error)]
pub enum TenantTemplateError {
    /// Creating, migrating, locking or dropping the template failed.
    #[error(transparent)]
    Store(#[from] ContextStoreError),
    /// The template has to be built again, and a session still holds it.
    #[error("the tenant template {template} has to be built again, and a session still holds it")]
    Held {
        /// The template's database.
        template: TenantDatabaseName,
    },
}

/// Make sure the server `direct_url` reaches holds the template the tenant
/// databases of `prefix` are copied from, at this build's schema and locked,
/// migrating it with the session defaults `defaults` and the migration pool's
/// `acquire_timeout`.
///
/// # Errors
///
/// Returns [`TenantTemplateError::Store`] when the template cannot be created,
/// migrated, locked or dropped, or its scheduler cannot be stopped, and
/// [`TenantTemplateError::Held`] when a template this build cannot bring
/// forward has a session that keeps it from being dropped.
pub(crate) async fn prepare_tenant_template(
    direct_url: &DatabaseUrl,
    prefix: &DbNamePrefix,
    defaults: &DatabaseDefaults,
    acquire_timeout: Duration,
) -> Result<TenantTemplate, TenantTemplateError> {
    let template = template_database_name(prefix);
    let creation = create_database_if_missing(direct_url, &template, DatabaseSource::ServerDefault).await?;
    match synchronize_template(direct_url, &template, defaults, acquire_timeout).await {
        Ok(()) => {}
        Err(error) if creation == DatabaseCreation::AlreadyPresent && is_schema_out_of_reach(&error) => {
            tracing::warn!(
                template = %template,
                %error,
                event = "tenant_template_rebuilt",
                "the tenant template could not be brought to this build's schema, so it is built again"
            );
            rebuild_template(direct_url, &template, defaults, acquire_timeout).await?;
        }
        Err(error) => return Err(error.into()),
    }
    tracing::info!(template = %template, event = "tenant_template_ready", "the tenant template is at this build's schema and locked");
    Ok(TenantTemplate { database: template })
}

/// Drop `template` and build it afresh.
async fn rebuild_template(
    direct_url: &DatabaseUrl,
    template: &TenantDatabaseName,
    defaults: &DatabaseDefaults,
    acquire_timeout: Duration,
) -> Result<(), TenantTemplateError> {
    match retire_template(direct_url, template).await? {
        DatabaseDrop::Dropped => {}
        DatabaseDrop::InUse => {
            return Err(TenantTemplateError::Held {
                template: template.clone(), // clone: the error outlives this borrow
            });
        }
    }
    create_database_if_missing(direct_url, template, DatabaseSource::ServerDefault).await?;
    Ok(synchronize_template(direct_url, template, defaults, acquire_timeout).await?)
}

/// Whether bringing the template forward failed on the schema it holds, which
/// a template built afresh does not carry: a migration that fails, or a stamp
/// or a migration ledger this build refuses.
const fn is_schema_out_of_reach(error: &ContextStoreError) -> bool {
    matches!(error, ContextStoreError::Migration(_) | ContextStoreError::SchemaContract(_))
}

#[cfg(test)]
mod tests {
    use crate::{
        error::ContextStoreError,
        schema_contract::{error::SchemaContractError, schema_version::SchemaVersion},
        tenant_template::is_schema_out_of_reach,
    };
    use sqlx::migrate::MigrateError;

    #[test]
    fn a_refused_stamp_or_a_failed_migration_rebuilds_the_template() {
        let ahead = SchemaContractError::SchemaVersionMismatch {
            expected: SchemaVersion::try_from(39).expect("a valid schema version"),
            found: SchemaVersion::try_from(40).expect("a valid schema version"),
        };
        assert!(is_schema_out_of_reach(&ContextStoreError::SchemaContract(ahead)));
        assert!(is_schema_out_of_reach(&ContextStoreError::Migration(MigrateError::VersionMismatch(33))));
    }

    #[test]
    fn a_refused_privilege_or_a_lost_connection_does_not_rebuild_the_template() {
        assert!(!is_schema_out_of_reach(&ContextStoreError::from(sqlx::Error::PoolTimedOut)));
        assert!(!is_schema_out_of_reach(&ContextStoreError::Saturated));
    }
}
