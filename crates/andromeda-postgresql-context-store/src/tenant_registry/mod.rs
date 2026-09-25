//! The `tenants` table, as the tenancy crate's registry and lifecycle ports.

mod statements;
/// The number the `tenants.status` column stores a tenant's status as.
pub mod status_code;

use crate::{
    error::ContextStoreError,
    tenant_database::{DatabaseUrl, create_database_if_missing, generate_db_name},
    tenant_migration::{configure_platform_database, migrate_platform_db, migrate_single_tenant_db},
    tenant_registry::{
        statements::{
            advisory_lock_query,
            default_tenant_upsert,
            named_tenant_database_query,
            registered_tenants_query,
            set_status_update,
            tenant_count_query,
            tenant_insert,
            tenant_record_query,
            tenant_records_page_query,
        },
        status_code::{status_of, stored_status},
    },
};
use andromeda_ngsi_ld_lexicon::tenant::TenantName;
use andromeda_postgresql_codes::tenant_status::TenantStatusCode;
use andromeda_postgresql_pool::{configure_pool_options, is_pool_saturated};
use andromeda_tenancy::{
    config::TenancyConfig,
    database::TenantDatabase,
    database_name::TenantDatabaseName,
    error::TenancyError,
    lifecycle::TenantLifecycle,
    status::TenantStatus,
    tenant_keyset::TenantKeyset,
    tenant_record::TenantRecord,
    tenant_ref::TenantRef,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sea_query::{PostgresQueryBuilder, error::Error as SeaQueryError};
use sea_query_sqlx::SqlxBinder;
use sqlx::{PgPool, Row, postgres::PgRow};
use std::time::Duration;

/// The tenant registry, backed by the platform database's `tenants` table.
///
/// Holds the main pool rather than borrowing one: the registry outlives every
/// tenant runtime and is the only thing that can answer which tenants exist
/// after they have all been evicted.
pub struct PostgresTenantRegistry {
    /// The platform database's request-path pool, where the registry lives and
    /// serves the default tenant. Reaches PostgreSQL through the pooler when one
    /// is configured.
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
    /// The PostgreSQL `synchronous_commit` value installed on each database with
    /// `ALTER DATABASE ... SET` at creation (e.g. `"on"`, `"off"`, `"local"`).
    synchronous_commit_setting: &'static str,
    /// How long a checkout waits for a connection before the migration and
    /// provisioning pools give up, matching the request path's acquire deadline.
    acquire_timeout: Duration,
}

impl PostgresTenantRegistry {
    /// Open the platform database, migrate it, and register the unnamed
    /// default tenant.
    ///
    /// # Errors
    ///
    /// Returns [`ContextStoreError::Internal`] when the base URL names no
    /// database or the default-tenant upsert cannot be assembled,
    /// [`ContextStoreError::Migration`] or [`ContextStoreError::SchemaContract`]
    /// when the platform database cannot be migrated to this binary's contract,
    /// and [`ContextStoreError::Database`] when the upsert fails.
    pub async fn connect(
        main_pool: PgPool,
        direct_url: DatabaseUrl,
        tenancy: TenancyConfig,
        synchronous_commit_setting: &'static str,
        acquire_timeout: Duration,
    ) -> Result<PostgresTenantRegistry, ContextStoreError> {
        let default_database = direct_url.db_name()?;
        let registry = PostgresTenantRegistry {
            main_pool,
            direct_url,
            default_database,
            tenancy,
            synchronous_commit_setting,
            acquire_timeout,
        };

        // Migrate the platform database over a direct, one-connection pool: the
        // migrator's session advisory lock is unsafe through a pooler, so it
        // never runs on `main_pool`.
        let platform_url = registry.direct_url.with_db_name(&registry.default_database);
        let platform_migration_pool = configure_pool_options(1, registry.acquire_timeout).connect(platform_url.as_str()).await?;
        let migration_result = match configure_platform_database(
            &platform_migration_pool,
            registry.default_database.as_str(),
            registry.synchronous_commit_setting,
        )
        .await
        {
            Ok(()) => migrate_platform_db(&platform_migration_pool).await,
            Err(error) => Err(error),
        };
        platform_migration_pool.close().await;
        migration_result?;

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
        let statement = default_tenant_upsert(self.default_database.as_str(), stored_status(TenantStatus::Active)).map_err(|error| statement_error(&error))?;
        let (sql, values) = statement.build_sqlx(PostgresQueryBuilder);
        sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values).execute(&self.main_pool).await?;
        Ok(())
    }
}

/// The failure a fleet-wide registry read becomes.
fn registry_unavailable(error: &sqlx::Error) -> TenancyError {
    TenancyError::RegistryUnavailable { detail: error.to_string() }
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

/// The failure a statement that could not be assembled becomes.
///
/// A column/value arity mismatch is the only cause, so it is an internal
/// invariant rather than a database failure.
fn statement_error(error: &SeaQueryError) -> ContextStoreError {
    ContextStoreError::Internal(error.to_string())
}

/// Build a [`TenantRecord`] from one registry row, projected as `name`, `status`,
/// `db_name`, `created_at`, `modified_at`.
///
/// A column the registry cannot present as a tenant name, database name or known
/// status is a registry corruption rather than a per-tenant fault, so it surfaces
/// as [`TenancyError::RegistryUnavailable`].
fn record_from_row(row: &PgRow) -> Result<TenantRecord, TenancyError> {
    let name: String = row.try_get("name").map_err(|error| registry_unavailable(&error))?;
    let status: i16 = row.try_get("status").map_err(|error| registry_unavailable(&error))?;
    let db_name: String = row.try_get("db_name").map_err(|error| registry_unavailable(&error))?;
    let created_at: DateTime<Utc> = row.try_get("created_at").map_err(|error| registry_unavailable(&error))?;
    let modified_at: DateTime<Utc> = row.try_get("modified_at").map_err(|error| registry_unavailable(&error))?;

    let tenant_name = TenantName::try_from(name).map_err(|error| TenancyError::RegistryUnavailable {
        detail: format!("the registry holds a name that is not a tenant name: {error}"),
    })?;
    let status = status_of(TenantStatusCode::try_from(status).map_err(|error| TenancyError::RegistryUnavailable {
        detail: format!("the registry holds an unrecognised tenant status: {error}"),
    })?);
    Ok(TenantRecord::new(
        tenant_name,
        status,
        TenantDatabaseName::from(db_name),
        created_at,
        modified_at,
    ))
}

#[async_trait]
impl TenantDatabase for PostgresTenantRegistry {
    fn default_database(&self) -> TenantDatabaseName {
        self.default_database.clone() // clone: the port hands back an owned name
    }

    async fn registered_tenants(&self) -> Result<Vec<TenantName>, TenancyError> {
        let (sql, values) = registered_tenants_query().build_sqlx(PostgresQueryBuilder);
        let names: Vec<String> = sqlx::query_scalar_with(sqlx::AssertSqlSafe(sql.as_str()), values)
            .fetch_all(&self.main_pool)
            .await
            .map_err(|error| registry_unavailable(&error))?;

        names
            .into_iter()
            .map(|name| {
                TenantName::try_from(name).map_err(|error| TenancyError::RegistryUnavailable {
                    detail: format!("the registry holds a name that is not a tenant name: {error}"),
                })
            })
            .collect()
    }

    async fn tenant_record(&self, name: &TenantName) -> Result<Option<TenantRecord>, TenancyError> {
        let (sql, values) = tenant_record_query(name.as_str()).build_sqlx(PostgresQueryBuilder);
        let row = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values)
            .fetch_optional(&self.main_pool)
            .await
            .map_err(|error| registry_read_failure(&error, name))?;
        row.as_ref().map(record_from_row).transpose()
    }

    async fn list_tenant_records(&self, keyset: &TenantKeyset, limit: i64) -> Result<(Vec<TenantRecord>, i64), TenancyError> {
        let (count_sql, count_values) = tenant_count_query().build_sqlx(PostgresQueryBuilder);
        let total: i64 = sqlx::query_scalar_with(sqlx::AssertSqlSafe(count_sql.as_str()), count_values)
            .fetch_one(&self.main_pool)
            .await
            .map_err(|error| registry_unavailable(&error))?;

        let (sql, values) = tenant_records_page_query(keyset, limit).build_sqlx(PostgresQueryBuilder);
        let rows = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values)
            .fetch_all(&self.main_pool)
            .await
            .map_err(|error| registry_unavailable(&error))?;
        let mut records = rows.iter().map(record_from_row).collect::<Result<Vec<TenantRecord>, TenancyError>>()?;
        // A backward or from-end page was read descending; present it ascending
        // so a page always reads in name order regardless of its direction.
        if matches!(keyset, TenantKeyset::Backward { .. } | TenantKeyset::FromEnd) {
            records.reverse();
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
                    detail: error.to_string(),
                }
            }
        })
    }
}

#[async_trait]
impl TenantLifecycle for PostgresTenantRegistry {
    async fn set_status(&self, name: &TenantName, status: TenantStatus) -> Result<(), TenancyError> {
        let (sql, values) = set_status_update(name.as_str(), stored_status(status)).build_sqlx(PostgresQueryBuilder);
        sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values)
            .execute(&self.main_pool)
            .await
            .map_err(|error| registry_read_failure(&error, name))?;
        Ok(())
    }
}

impl PostgresTenantRegistry {
    /// Create the tenant's database and register it, once.
    ///
    /// The advisory lock is what makes this safe against two requests naming
    /// the same unknown tenant: both take it, the first creates, the second
    /// finds the row the first inserted.
    async fn provision_inner(&self, name: &TenantName) -> Result<TenantDatabaseName, ContextStoreError> {
        let mut tx = self.main_pool.begin().await?;
        let (lock_sql, lock_values) = advisory_lock_query(name.as_str()).build_sqlx(PostgresQueryBuilder);
        sqlx::query_with(sqlx::AssertSqlSafe(lock_sql.as_str()), lock_values).execute(&mut *tx).await?;

        let (lookup_sql, lookup_values) = named_tenant_database_query(name.as_str()).build_sqlx(PostgresQueryBuilder);
        if let Some(existing) = sqlx::query_scalar_with::<_, String, _>(sqlx::AssertSqlSafe(lookup_sql.as_str()), lookup_values)
            .fetch_optional(&mut *tx)
            .await?
        {
            let db_name = TenantDatabaseName::from(existing);
            tx.commit().await?;
            return Ok(db_name);
        }

        let db_name = generate_db_name(&self.tenancy.tenant_db_prefix, name);
        create_database_if_missing(&self.direct_url, &db_name, self.synchronous_commit_setting).await?;

        let statement = tenant_insert(name.as_str(), db_name.as_str(), stored_status(TenantStatus::Active)).map_err(|error| statement_error(&error))?;
        let (sql, values) = statement.build_sqlx(PostgresQueryBuilder);
        sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values).execute(&mut *tx).await?;
        tx.commit().await?;

        let tenant_url = self.direct_url.with_db_name(&db_name);
        migrate_single_tenant_db(&tenant_url, self.acquire_timeout).await?;

        Ok(db_name)
    }
}
