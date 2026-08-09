//! The `tenants` table, as the tenancy crate's registry port.

use crate::{
    error::ContextStoreError,
    tenant_database::{DatabaseUrl, create_database_if_missing, generate_db_name, migrate_main_db, migrate_single_tenant_db},
};
use andromeda_ngsi_ld_lexicon::tenant::TenantName;
use andromeda_tenancy::{config::TenancyConfig, database::TenantDatabase, database_name::TenantDatabaseName, error::TenancyError, status::TenantStatus};
use async_trait::async_trait;
use sqlx::{PgPool, Row};

/// The tenant registry, backed by the platform database's `tenants` table.
///
/// Holds the main pool rather than borrowing one: the registry outlives every
/// tenant runtime and is the only thing that can answer which tenants exist
/// after they have all been evicted.
pub struct PostgresTenantRegistry {
    /// The platform database, where the registry lives.
    main_pool: PgPool,
    /// The URL every per-tenant database is derived from.
    database_url: DatabaseUrl,
    /// The database the unnamed default tenant uses.
    default_database: TenantDatabaseName,
    /// Prefix and pool size for provisioning.
    tenancy: TenancyConfig,
    /// The PostgreSQL `synchronous_commit` setting value applied to every
    /// pool opened for a migration (e.g. `"on"`, `"off"`, `"local"`).
    synchronous_commit_setting: &'static str,
}

impl PostgresTenantRegistry {
    /// Open the platform database, migrate it, and register the unnamed
    /// default tenant.
    pub async fn connect(
        main_pool: PgPool,
        database_url: DatabaseUrl,
        tenancy: TenancyConfig,
        synchronous_commit_setting: &'static str,
    ) -> Result<PostgresTenantRegistry, ContextStoreError> {
        let default_database = database_url.db_name()?;
        let registry = PostgresTenantRegistry {
            main_pool,
            database_url,
            default_database,
            tenancy,
            synchronous_commit_setting,
        };

        migrate_main_db(&registry.main_pool).await?;
        registry.register_default_tenant().await?;
        Ok(registry)
    }

    /// The platform pool, for the health indicator and the version service.
    pub const fn main_pool(&self) -> &PgPool {
        &self.main_pool
    }

    /// Record the unnamed default tenant, so it appears in the registry like
    /// any other.
    async fn register_default_tenant(&self) -> Result<(), ContextStoreError> {
        sqlx::query(
            r#"
            INSERT INTO tenants (name, is_unnamed_default, db_name, status, created_at, modified_at)
            VALUES (NULL, TRUE, $1, $2, NOW(), NOW())
            ON CONFLICT (db_name) DO UPDATE SET modified_at = NOW()
            "#,
        )
        .bind(self.default_database.as_str())
        .bind(TenantStatus::Active.to_string())
        .execute(&self.main_pool)
        .await?;

        Ok(())
    }
}

/// The failure a registry read becomes.
fn registry_unavailable(error: &sqlx::Error) -> TenancyError {
    TenancyError::RegistryUnavailable { detail: error.to_string() }
}

#[async_trait]
impl TenantDatabase for PostgresTenantRegistry {
    fn default_database(&self) -> TenantDatabaseName {
        self.default_database.clone() // clone: the port hands back an owned name
    }

    async fn registered_tenants(&self) -> Result<Vec<TenantName>, TenancyError> {
        let names: Vec<String> = sqlx::query_scalar("SELECT name FROM tenants WHERE is_unnamed_default = FALSE AND name IS NOT NULL ORDER BY name")
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

    async fn lookup(&self, name: &TenantName) -> Result<Option<TenantDatabaseName>, TenancyError> {
        let row = sqlx::query("SELECT db_name FROM tenants WHERE name = $1 AND is_unnamed_default = FALSE")
            .bind(name.as_str())
            .fetch_optional(&self.main_pool)
            .await
            .map_err(|error| registry_unavailable(&error))?;
        Ok(row.map(|row| TenantDatabaseName::from(row.get::<String, _>("db_name"))))
    }

    async fn provision(&self, name: &TenantName) -> Result<TenantDatabaseName, TenancyError> {
        self.provision_inner(name).await.map_err(|error| TenancyError::ProvisioningFailed {
            name: name.clone(), // clone: the error outlives this borrow
            detail: error.to_string(),
        })
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
        sqlx::query("SELECT pg_advisory_xact_lock(hashtext($1))")
            .bind(name.as_str())
            .execute(&mut *tx)
            .await?;

        if let Some(existing) = sqlx::query("SELECT db_name FROM tenants WHERE name = $1")
            .bind(name.as_str())
            .fetch_optional(&mut *tx)
            .await?
        {
            let db_name = TenantDatabaseName::from(existing.get::<String, _>("db_name"));
            tx.commit().await?;
            return Ok(db_name);
        }

        let db_name = generate_db_name(&self.tenancy.tenant_db_prefix, name);
        create_database_if_missing(&self.database_url, &db_name).await?;

        sqlx::query(
            r#"
            INSERT INTO tenants (name, is_unnamed_default, db_name, status, created_at, modified_at)
            VALUES ($1, FALSE, $2, $3, NOW(), NOW())
            "#,
        )
        .bind(name.as_str())
        .bind(db_name.as_str())
        .bind(TenantStatus::Active.to_string())
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;

        let tenant_url = self.database_url.with_db_name(&db_name);
        migrate_single_tenant_db(&tenant_url, self.tenancy.tenant_max_connections, self.synchronous_commit_setting).await?;

        Ok(db_name)
    }
}
