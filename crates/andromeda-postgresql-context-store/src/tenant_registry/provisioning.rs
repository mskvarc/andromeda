//! Bringing a named tenant into existence: its provisional registry row, then
//! its database.
//!
//! The two steps wait on different pools, the platform pool and the
//! one-connection pool the tenant's migrations open, so a saturated pool is
//! reported as the step that waited on it.

use crate::{
    error::ContextStoreError,
    store_fault::persistence_failure,
    tenant_database::create_database_if_missing,
    tenant_database_naming::derive_database_name,
    tenant_migration::migrate_single_tenant_db,
    tenant_registry::{
        PostgresTenantRegistry,
        saturated,
        statements::{advisory_lock_query, named_tenant_standing_query, provisional_touch, tenant_insert},
        status_code::{StoredStanding, standing_of},
    },
};
use andromeda_ngsi_ld_lexicon::{tenant::TenantName, tenant_database_name::TenantDatabaseName};
use andromeda_postgresql_codes::tenant_status::TenantStatusCode;
use andromeda_postgresql_seaquery::statement::Statement;
use andromeda_tenancy::{error::TenancyError, saturated_wait::SaturatedWait};

/// What the registration transaction found or wrote for the tenant.
enum Registration {
    /// The tenant was already established; its database needs nothing.
    Established(TenantDatabaseName),
    /// The tenant is provisional, newly or from an interrupted provisioning;
    /// its database must be created and migrated.
    Provisional(TenantDatabaseName),
}

impl PostgresTenantRegistry {
    /// Register the tenant as provisional, then create and migrate its
    /// database; for a tenant already registered, make sure its database is
    /// created and migrated.
    ///
    /// The advisory lock serialises the registry decision: of two requests
    /// naming the same unknown tenant, the first inserts the row, the second
    /// finds it. The row commits before the database is created, so a crash
    /// can leave a provisional row with no database, which the next
    /// provisioning or the retirement of the tenant settles, but never a
    /// database with no row. Creating and migrating are idempotent, so the
    /// second request preparing the same database is harmless.
    pub(super) async fn provision_tenant(&self, name: &TenantName) -> Result<TenantDatabaseName, TenancyError> {
        let registration = self
            .register_provisional(name)
            .await
            .map_err(|error| provisioning_failure(&error, name, SaturatedWait::RegistrationTransaction))?;
        match registration {
            Registration::Established(database) => Ok(database),
            Registration::Provisional(database) => {
                self.prepare_database(&database)
                    .await
                    .map_err(|error| provisioning_failure(&error, name, SaturatedWait::MigrationPool))?;
                Ok(database)
            }
        }
    }

    /// Record the tenant as provisional under its advisory lock, or find the
    /// row a previous request wrote.
    async fn register_provisional(&self, name: &TenantName) -> Result<Registration, ContextStoreError> {
        let mut tx = self.main_pool.begin().await?;
        advisory_lock_query(name.as_str()).to_query().execute(&mut *tx).await?;

        let existing = named_tenant_standing_query(name.as_str())
            .to_query_as::<(String, i16)>()
            .fetch_optional(&mut *tx)
            .await?;
        let registration = if let Some((database_name, status)) = existing {
            match standing_of(TenantStatusCode::try_from(status).map_err(ContextStoreError::StoredTenantStatus)?) {
                StoredStanding::Established(_) => Registration::Established(TenantDatabaseName::from(database_name)),
                StoredStanding::Provisional => {
                    provisional_touch(name.as_str()).to_query().execute(&mut *tx).await?;
                    Registration::Provisional(TenantDatabaseName::from(database_name))
                }
            }
        } else {
            let database_name = derive_database_name(&self.databases.name_prefix, name);
            let statement = tenant_insert(name.as_str(), database_name.as_str(), TenantStatusCode::Provisional)?;
            statement.to_query().execute(&mut *tx).await?;
            Registration::Provisional(database_name)
        };
        tx.commit().await?;
        Ok(registration)
    }

    /// Create the tenant's database if it is missing and migrate it.
    ///
    /// `CREATE DATABASE` and the extension update run over a plain connection,
    /// so the only pool this step waits on is the one its migrations open.
    async fn prepare_database(&self, database_name: &TenantDatabaseName) -> Result<(), ContextStoreError> {
        create_database_if_missing(&self.direct_url, database_name).await?;
        let tenant_url = self.direct_url.with_database_name(database_name);
        migrate_single_tenant_db(&tenant_url, &self.defaults, self.acquire_timeout).await
    }
}

/// The failure a provisioning step becomes: a saturated pool is backpressure,
/// shed as a 503 naming `wait`; every other failure is a genuine provisioning
/// fault.
fn provisioning_failure(error: &ContextStoreError, name: &TenantName, wait: SaturatedWait) -> TenancyError {
    if matches!(error, ContextStoreError::Saturated) {
        saturated(name, wait)
    } else {
        TenancyError::ProvisioningFailed {
            name: name.clone(), // clone: the error outlives this borrow
            cause: persistence_failure(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{error::ContextStoreError, tenant_registry::provisioning::provisioning_failure};
    use andromeda_ngsi_ld_lexicon::tenant::{InvalidTenantName, TenantName};
    use andromeda_tenancy::{error::TenancyError, saturated_wait::SaturatedWait};

    #[test]
    fn a_saturated_provisioning_step_names_the_wait_of_that_step() -> Result<(), InvalidTenantName> {
        let name = TenantName::try_from("acme".to_owned())?;
        for wait in [SaturatedWait::RegistrationTransaction, SaturatedWait::MigrationPool] {
            let refused = provisioning_failure(&ContextStoreError::from(sqlx::Error::PoolTimedOut), &name, wait);
            assert!(
                matches!(refused, TenancyError::Saturated { wait: reported, .. } if reported == wait),
                "{wait}: {refused:?}"
            );
        }
        Ok(())
    }

    #[test]
    fn a_provisioning_fault_is_not_shed_as_saturated() -> Result<(), InvalidTenantName> {
        let name = TenantName::try_from("acme".to_owned())?;
        let fault = provisioning_failure(&ContextStoreError::from(sqlx::Error::RowNotFound), &name, SaturatedWait::MigrationPool);
        assert!(matches!(fault, TenancyError::ProvisioningFailed { .. }), "{fault:?}");
        Ok(())
    }
}
