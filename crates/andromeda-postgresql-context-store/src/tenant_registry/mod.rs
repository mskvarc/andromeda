//! The `tenants` table, as the tenancy crate's registry and lifecycle ports.

mod occupancy;
mod settlement;
mod statements;
/// The number the `tenants.status` column stores a tenant's status as.
pub mod status_code;

use crate::{
    database_defaults::{DatabaseDefaults, reconcile_database_defaults},
    error::ContextStoreError,
    extension_update::update_extensions,
    store_fault::{persistence_failure, undecodable},
    tenant_database::{DatabaseUrl, create_database_if_missing, generate_database_name},
    tenant_migration::{migrate_platform_db, migrate_single_tenant_db, open_schema_change_pool},
    tenant_registry::{
        statements::{
            advisory_lock_query,
            default_tenant_upsert,
            named_tenant_standing_query,
            provisional_tenants_query,
            provisional_touch,
            registered_tenant_query,
            registered_tenants_query,
            set_status_update,
            tenant_count_query,
            tenant_insert,
            tenant_records_page_query,
        },
        status_code::{StoredStanding, standing_of, stored_status},
    },
};
use andromeda_ngsi_ld_lexicon::tenant::TenantName;
use andromeda_persistence_failure::persistence_failure::PersistenceFailure;
use andromeda_postgresql_codes::tenant_status::TenantStatusCode;
use andromeda_postgresql_pool::saturation::is_pool_saturated;
use andromeda_postgresql_seaquery::statement::Statement;
use andromeda_tenancy::{
    config::TenancyConfig,
    database::TenantDatabase,
    database_name::TenantDatabaseName,
    error::TenancyError,
    lifecycle::TenantLifecycle,
    provisional::{Establishment, Retirement},
    registered_tenant::RegisteredTenant,
    status::TenantStatus,
    tenant_page_start::TenantPageStart,
    tenant_record::TenantRecord,
    tenant_ref::TenantRef,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::{
    PgPool,
    Row,
    postgres::{PgPoolOptions, PgRow},
};
use std::time::Duration;

/// The tenant registry, backed by the platform database's `tenants` table.
///
/// Holds the main pool rather than borrowing one: the registry outlives every
/// tenant runtime and is the only thing that can answer which tenants exist
/// after they have all been evicted.
pub struct PostgresTenantRegistry {
    /// The platform database's pool, where the registry lives, connected
    /// straight to PostgreSQL even when a pooler is configured: the platform is one database, so a pooler has nothing to
    /// multiplex for it, and provisioning holds a transaction on it across
    /// `CREATE DATABASE`.
    main_pool: PgPool,
    /// The straight-to-PostgreSQL base URL every per-tenant database is derived
    /// from for provisioning and migration. Never a pooler: `CREATE DATABASE`
    /// cannot run in a transaction and the migrator holds a session advisory
    /// lock, both of which a transaction-pooling proxy would break.
    direct_url: DatabaseUrl,
    /// The database the unnamed default tenant uses.
    default_database: TenantDatabaseName,
    /// Prefix and pool size for provisioning.
    tenancy: TenancyConfig,
    /// How long a checkout waits for a connection before the migration and
    /// provisioning pools give up, matching the request path's acquire deadline.
    acquire_timeout: Duration,
}

impl PostgresTenantRegistry {
    /// Open the platform database, install its session defaults, migrate it,
    /// open the platform pool, and register the unnamed default tenant.
    ///
    /// The platform pool opens after the defaults are installed, so none of its
    /// sessions starts with the values they replace.
    ///
    /// # Errors
    ///
    /// Returns [`ContextStoreError::DatabaseUrlUnnamed`] when the base URL names
    /// no database, [`ContextStoreError::StatementBuild`] when the
    /// default-tenant upsert cannot be assembled,
    /// [`ContextStoreError::Migration`] or [`ContextStoreError::SchemaContract`]
    /// when the platform database cannot be migrated to this binary's contract,
    /// and [`ContextStoreError::Database`] when the defaults cannot be
    /// installed, the platform pool cannot connect or the upsert fails.
    pub async fn connect(
        main_pool_options: PgPoolOptions,
        direct_url: DatabaseUrl,
        tenancy: TenancyConfig,
        defaults: &DatabaseDefaults,
        acquire_timeout: Duration,
    ) -> Result<PostgresTenantRegistry, ContextStoreError> {
        let default_database = direct_url.database_name()?;

        // Migrate the platform database over a direct, one-connection pool: the
        // migrator's session advisory lock is unsafe through a pooler, so it
        // never runs on `main_pool`. Its extensions are updated first, before
        // any statement loads the installed TimescaleDB into a connection.
        let platform_url = direct_url.with_database_name(&default_database);
        update_extensions(&platform_url).await?;
        let platform_migration_pool = open_schema_change_pool(&platform_url, acquire_timeout).await?;
        let migration_result = match reconcile_database_defaults(&platform_migration_pool, defaults).await {
            Ok(_) => migrate_platform_db(&platform_migration_pool).await,
            Err(error) => Err(error),
        };
        platform_migration_pool.close().await;
        migration_result?;

        let registry = PostgresTenantRegistry {
            main_pool: main_pool_options.connect(platform_url.as_str()).await?,
            direct_url,
            default_database,
            tenancy,
            acquire_timeout,
        };
        registry.register_default_tenant().await?;
        Ok(registry)
    }

    /// The platform pool, for the health indicator and the version service.
    #[must_use]
    pub const fn main_pool(&self) -> &PgPool {
        &self.main_pool
    }

    /// Record the unnamed default tenant, so it appears in the registry like
    /// any other.
    async fn register_default_tenant(&self) -> Result<(), ContextStoreError> {
        let statement = default_tenant_upsert(self.default_database.as_str(), stored_status(TenantStatus::Active))?;
        statement.to_query().execute(&self.main_pool).await?;
        Ok(())
    }
}

/// The failure a fleet-wide registry read becomes.
#[track_caller]
fn registry_unavailable(error: &sqlx::Error) -> TenancyError {
    TenancyError::RegistryUnavailable(persistence_failure(error))
}

/// The failure a per-tenant registry read becomes, telling a saturated backend
/// pool (backpressure, shed as a 503) apart from a genuine registry fault (500).
fn registry_read_failure(error: &sqlx::Error, name: &TenantName) -> TenancyError {
    if is_pool_saturated(error) {
        TenancyError::Saturated {
            tenant: TenantRef::Named(name.clone()),
        } // clone: the error outlives this borrow
    } else {
        registry_unavailable(error)
    }
}

/// The failure a per-tenant registry write becomes, telling a saturated backend
/// pool (backpressure, shed as a 503) apart from a genuine registry fault (500).
fn registry_write_failure(error: &ContextStoreError, name: &TenantName) -> TenancyError {
    if matches!(error, ContextStoreError::Saturated) {
        TenancyError::Saturated {
            tenant: TenantRef::Named(name.clone()),
        } // clone: the error outlives this borrow
    } else {
        TenancyError::RegistryUnavailable(persistence_failure(error))
    }
}

/// Build a [`RegisteredTenant`] from one registry row, projected as `name`,
/// `status`, `database_name`, `created_at`, `modified_at`.
///
/// A column the registry cannot present as a tenant name, database name or known
/// status is a registry corruption rather than a per-tenant fault, so it surfaces
/// as [`TenancyError::RegistryUnavailable`].
fn registered_from_row(row: &PgRow) -> Result<RegisteredTenant, TenancyError> {
    let name: String = row.try_get("name").map_err(|error| registry_unavailable(&error))?;
    let status: i16 = row.try_get("status").map_err(|error| registry_unavailable(&error))?;
    let database_name: String = row.try_get("database_name").map_err(|error| registry_unavailable(&error))?;
    let created_at: DateTime<Utc> = row.try_get("created_at").map_err(|error| registry_unavailable(&error))?;
    let modified_at: DateTime<Utc> = row.try_get("modified_at").map_err(|error| registry_unavailable(&error))?;

    let tenant_name = TenantName::try_from(name).map_err(|error| TenancyError::RegistryUnavailable(undecodable(&error)))?;
    let database = TenantDatabaseName::from(database_name);
    Ok(
        match standing_of(TenantStatusCode::try_from(status).map_err(|error| TenancyError::RegistryUnavailable(undecodable(&error)))?) {
            StoredStanding::Provisional => RegisteredTenant::Provisional { database },
            StoredStanding::Established(status) => RegisteredTenant::Established(TenantRecord::new(tenant_name, status, database, created_at, modified_at)),
        },
    )
}

/// The provisional row a collection page holds against its contract.
#[derive(Debug, thiserror::Error)]
#[error("a tenant collection page held a provisional tenant")]
struct ProvisionalInCollection;

/// Build the [`TenantRecord`] of one row of a collection page, which reads
/// established tenants only.
fn listed_record_from_row(row: &PgRow) -> Result<TenantRecord, TenancyError> {
    match registered_from_row(row)? {
        RegisteredTenant::Established(record) => Ok(record),
        RegisteredTenant::Provisional { .. } => Err(TenancyError::RegistryUnavailable(
            PersistenceFailure::ContractBroken.record(&ProvisionalInCollection),
        )),
    }
}

/// Decode a list of registry names.
fn names_from(names: Vec<String>) -> Result<Vec<TenantName>, TenancyError> {
    names
        .into_iter()
        .map(|name| TenantName::try_from(name).map_err(|error| TenancyError::RegistryUnavailable(undecodable(&error))))
        .collect()
}

#[async_trait]
impl TenantDatabase for PostgresTenantRegistry {
    fn default_database(&self) -> TenantDatabaseName {
        self.default_database.clone() // clone: the port hands back an owned name
    }

    async fn registered_tenants(&self) -> Result<Vec<TenantName>, TenancyError> {
        let names: Vec<String> = registered_tenants_query()
            .to_query_scalar()
            .fetch_all(&self.main_pool)
            .await
            .map_err(|error| registry_unavailable(&error))?;
        names_from(names)
    }

    async fn registered_tenant(&self, name: &TenantName) -> Result<Option<RegisteredTenant>, TenancyError> {
        let row = registered_tenant_query(name.as_str())
            .to_query()
            .fetch_optional(&self.main_pool)
            .await
            .map_err(|error| registry_read_failure(&error, name))?;
        row.as_ref().map(registered_from_row).transpose()
    }

    async fn list_tenant_records(&self, start: &TenantPageStart, limit: u32) -> Result<(Vec<TenantRecord>, i64), TenancyError> {
        let total: i64 = tenant_count_query()
            .to_query_scalar()
            .fetch_one(&self.main_pool)
            .await
            .map_err(|error| registry_unavailable(&error))?;

        let rows = tenant_records_page_query(start, limit)
            .to_query()
            .fetch_all(&self.main_pool)
            .await
            .map_err(|error| registry_unavailable(&error))?;
        let mut records = rows.iter().map(listed_record_from_row).collect::<Result<Vec<TenantRecord>, TenancyError>>()?;
        // A backward or from-end page was read descending; present it ascending
        // so a page always reads in name order regardless of its direction.
        match start {
            TenantPageStart::Backward { .. } | TenantPageStart::FromEnd => records.reverse(),
            TenantPageStart::Offset(_) | TenantPageStart::Forward { .. } => {}
        }
        Ok((records, total))
    }

    async fn provision(&self, name: &TenantName) -> Result<TenantDatabaseName, TenancyError> {
        self.provision_inner(name).await.map_err(|error| {
            // A saturated backend pool while provisioning is backpressure, shed
            // as a 503; every other failure is a genuine provisioning fault.
            if matches!(error, ContextStoreError::Saturated) {
                TenancyError::Saturated {
                    tenant: TenantRef::Named(name.clone()),
                } // clone: the error outlives this borrow
            } else {
                TenancyError::ProvisioningFailed {
                    name: name.clone(), // clone: the error outlives this borrow
                    cause: persistence_failure(&error),
                }
            }
        })
    }

    async fn establish(&self, name: &TenantName) -> Result<Establishment, TenancyError> {
        self.establish_inner(name).await.map_err(|error| registry_write_failure(&error, name))
    }

    async fn provisional_tenants(&self) -> Result<Vec<TenantName>, TenancyError> {
        let names: Vec<String> = provisional_tenants_query()
            .to_query_scalar()
            .fetch_all(&self.main_pool)
            .await
            .map_err(|error| registry_unavailable(&error))?;
        names_from(names)
    }

    async fn retire_provisional(&self, name: &TenantName, idle_for: Duration) -> Result<Retirement, TenancyError> {
        self.retire_inner(name, idle_for).await.map_err(|error| registry_write_failure(&error, name))
    }
}

#[async_trait]
impl TenantLifecycle for PostgresTenantRegistry {
    async fn set_status(&self, name: &TenantName, status: TenantStatus) -> Result<(), TenancyError> {
        set_status_update(name.as_str(), stored_status(status))
            .to_query()
            .execute(&self.main_pool)
            .await
            .map_err(|error| registry_read_failure(&error, name))?;
        Ok(())
    }
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
    async fn provision_inner(&self, name: &TenantName) -> Result<TenantDatabaseName, ContextStoreError> {
        let mut tx = self.main_pool.begin().await?;
        advisory_lock_query(name.as_str()).to_query().execute(&mut *tx).await?;

        let existing = named_tenant_standing_query(name.as_str())
            .to_query_as::<(String, i16)>()
            .fetch_optional(&mut *tx)
            .await?;
        let database_name = if let Some((database_name, status)) = existing {
            match standing_of(TenantStatusCode::try_from(status).map_err(ContextStoreError::StoredTenantStatus)?) {
                StoredStanding::Established(_) => {
                    tx.commit().await?;
                    return Ok(TenantDatabaseName::from(database_name));
                }
                StoredStanding::Provisional => {
                    provisional_touch(name.as_str()).to_query().execute(&mut *tx).await?;
                    TenantDatabaseName::from(database_name)
                }
            }
        } else {
            let database_name = generate_database_name(&self.tenancy.tenant_db_prefix, name);
            let statement = tenant_insert(name.as_str(), database_name.as_str(), TenantStatusCode::Provisional)?;
            statement.to_query().execute(&mut *tx).await?;
            database_name
        };
        tx.commit().await?;

        create_database_if_missing(&self.direct_url, &database_name).await?;
        let tenant_url = self.direct_url.with_database_name(&database_name);
        migrate_single_tenant_db(&tenant_url, self.acquire_timeout).await?;

        Ok(database_name)
    }
}
