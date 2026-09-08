//! The `tenants` table, as the tenancy crate's registry port.

use crate::{
    error::ContextStoreError,
    tenant_database::{DatabaseUrl, create_database_if_missing, generate_db_name, migrate_main_db, migrate_single_tenant_db},
};
use andromeda_ngsi_ld_lexicon::tenant::TenantName;
use andromeda_postgresql_schema::tenancy::Tenants;
use andromeda_postgresql_seaquery::advisory_lock::advisory_xact_lock;
use andromeda_tenancy::{config::TenancyConfig, database::TenantDatabase, database_name::TenantDatabaseName, error::TenancyError, status::TenantStatus};
use async_trait::async_trait;
use sea_query::{
    Expr,
    ExprTrait,
    InsertStatement,
    Keyword,
    OnConflict,
    Order,
    PostgresQueryBuilder,
    Query,
    SelectStatement,
    SimpleExpr,
    error::Error as SeaQueryError,
};
use sea_query_sqlx::SqlxBinder;
use sqlx::PgPool;

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
    #[must_use]
    pub const fn main_pool(&self) -> &PgPool {
        &self.main_pool
    }

    /// Record the unnamed default tenant, so it appears in the registry like
    /// any other.
    async fn register_default_tenant(&self) -> Result<(), ContextStoreError> {
        let statement = default_tenant_upsert(self.default_database.as_str(), &TenantStatus::Active.to_string()).map_err(|error| statement_error(&error))?;
        let (sql, values) = statement.build_sqlx(PostgresQueryBuilder);
        sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values).execute(&self.main_pool).await?;
        Ok(())
    }
}

/// The failure a registry read becomes.
fn registry_unavailable(error: &sqlx::Error) -> TenancyError {
    TenancyError::RegistryUnavailable { detail: error.to_string() }
}

/// The failure a statement that could not be assembled becomes.
///
/// A column/value arity mismatch is the only cause, so it is an internal
/// invariant rather than a database failure.
fn statement_error(error: &SeaQueryError) -> ContextStoreError {
    ContextStoreError::Internal(error.to_string())
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

    async fn lookup(&self, name: &TenantName) -> Result<Option<TenantDatabaseName>, TenancyError> {
        let (sql, values) = named_tenant_database_query(name.as_str(), NamedTenantScope::ExcludeDefault).build_sqlx(PostgresQueryBuilder);
        let db_name: Option<String> = sqlx::query_scalar_with(sqlx::AssertSqlSafe(sql.as_str()), values)
            .fetch_optional(&self.main_pool)
            .await
            .map_err(|error| registry_unavailable(&error))?;
        Ok(db_name.map(TenantDatabaseName::from))
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
        let (lock_sql, lock_values) = advisory_lock_query(name.as_str()).build_sqlx(PostgresQueryBuilder);
        sqlx::query_with(sqlx::AssertSqlSafe(lock_sql.as_str()), lock_values).execute(&mut *tx).await?;

        let (lookup_sql, lookup_values) = named_tenant_database_query(name.as_str(), NamedTenantScope::AnyRow).build_sqlx(PostgresQueryBuilder);
        if let Some(existing) = sqlx::query_scalar_with::<_, String, _>(sqlx::AssertSqlSafe(lookup_sql.as_str()), lookup_values)
            .fetch_optional(&mut *tx)
            .await?
        {
            let db_name = TenantDatabaseName::from(existing);
            tx.commit().await?;
            return Ok(db_name);
        }

        let db_name = generate_db_name(&self.tenancy.tenant_db_prefix, name);
        create_database_if_missing(&self.database_url, &db_name).await?;

        let statement = tenant_insert(name.as_str(), db_name.as_str(), &TenantStatus::Active.to_string()).map_err(|error| statement_error(&error))?;
        let (sql, values) = statement.build_sqlx(PostgresQueryBuilder);
        sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values).execute(&mut *tx).await?;
        tx.commit().await?;

        let tenant_url = self.database_url.with_db_name(&db_name);
        migrate_single_tenant_db(&tenant_url, self.tenancy.tenant_max_connections, self.synchronous_commit_setting).await?;

        Ok(db_name)
    }
}

/// Whether a `db_name` lookup restricts to named tenants or reads any row.
///
/// The port's `lookup` excludes the unnamed default so a request for a named
/// tenant can never resolve to it; provisioning reads any row, since it is
/// checking whether the row it is about to insert already exists.
#[derive(Clone, Copy)]
enum NamedTenantScope {
    /// Exclude the unnamed default tenant.
    ExcludeDefault,
    /// Match any row with the name.
    AnyRow,
}

/// The names of every named tenant, in name order.
fn registered_tenants_query() -> SelectStatement {
    Query::select()
        .column(Tenants::Name)
        .from(Tenants::Table)
        .and_where(Expr::col(Tenants::IsUnnamedDefault).eq(false))
        .and_where(Expr::col(Tenants::Name).is_not_null())
        .order_by(Tenants::Name, Order::Asc)
        .to_owned()
}

/// The database name of the tenant called `name`, under `scope`.
fn named_tenant_database_query(name: &str, scope: NamedTenantScope) -> SelectStatement {
    let mut query = Query::select();
    query.column(Tenants::DbName).from(Tenants::Table).and_where(Expr::col(Tenants::Name).eq(name));
    match scope {
        NamedTenantScope::ExcludeDefault => {
            query.and_where(Expr::col(Tenants::IsUnnamedDefault).eq(false));
        }
        NamedTenantScope::AnyRow => {}
    }
    query.clone()
}

/// Take the transaction advisory lock keyed on `name`.
fn advisory_lock_query(name: &str) -> SelectStatement {
    Query::select().expr(advisory_xact_lock(Expr::val(name))).to_owned()
}

/// Insert the unnamed default tenant, refreshing its timestamp if it exists.
///
/// The conflict target is `db_name`, whose unique index is what keeps a restart
/// from inserting a second default row.
fn default_tenant_upsert(db_name: &str, status: &str) -> Result<InsertStatement, SeaQueryError> {
    let mut statement = Query::insert();
    statement
        .into_table(Tenants::Table)
        .columns([
            Tenants::Name,
            Tenants::IsUnnamedDefault,
            Tenants::DbName,
            Tenants::Status,
            Tenants::CreatedAt,
            Tenants::ModifiedAt,
        ])
        .values([
            SimpleExpr::Keyword(Keyword::Null),
            true.into(),
            db_name.into(),
            status.into(),
            Expr::current_timestamp(),
            Expr::current_timestamp(),
        ])?
        .on_conflict(
            OnConflict::column(Tenants::DbName)
                .values([(Tenants::ModifiedAt, Expr::current_timestamp())])
                .to_owned(),
        );
    Ok(statement.clone())
}

/// Insert a named tenant.
fn tenant_insert(name: &str, db_name: &str, status: &str) -> Result<InsertStatement, SeaQueryError> {
    let mut statement = Query::insert();
    statement
        .into_table(Tenants::Table)
        .columns([
            Tenants::Name,
            Tenants::IsUnnamedDefault,
            Tenants::DbName,
            Tenants::Status,
            Tenants::CreatedAt,
            Tenants::ModifiedAt,
        ])
        .values([
            name.into(),
            false.into(),
            db_name.into(),
            status.into(),
            Expr::current_timestamp(),
            Expr::current_timestamp(),
        ])?;
    Ok(statement.clone())
}

#[cfg(test)]
mod tests {
    use crate::tenant_registry::{
        NamedTenantScope,
        advisory_lock_query,
        default_tenant_upsert,
        named_tenant_database_query,
        registered_tenants_query,
        tenant_insert,
    };
    use sea_query::{PostgresQueryBuilder, SelectStatement};

    fn select_sql(statement: &SelectStatement) -> String {
        statement.to_string(PostgresQueryBuilder)
    }

    #[test]
    fn registered_tenants_lists_named_tenants_in_order() {
        let sql = select_sql(&registered_tenants_query());
        assert!(sql.contains("SELECT \"name\" FROM \"tenants\""), "{sql}");
        assert!(sql.contains("\"is_unnamed_default\" ="), "{sql}");
        assert!(sql.contains("\"name\" IS NOT NULL"), "{sql}");
        assert!(sql.contains("ORDER BY \"name\" ASC"), "{sql}");
    }

    #[test]
    fn the_port_lookup_excludes_the_unnamed_default() {
        let sql = select_sql(&named_tenant_database_query("acme", NamedTenantScope::ExcludeDefault));
        assert!(sql.contains("SELECT \"db_name\" FROM \"tenants\""), "{sql}");
        assert!(sql.contains("\"name\" ="), "{sql}");
        assert!(sql.contains("\"is_unnamed_default\""), "{sql}");
    }

    #[test]
    fn the_provisioning_lookup_reads_any_row() {
        let sql = select_sql(&named_tenant_database_query("acme", NamedTenantScope::AnyRow));
        assert!(sql.contains("\"name\" ="), "{sql}");
        assert!(!sql.contains("is_unnamed_default"), "provisioning must not exclude the default row: {sql}");
    }

    #[test]
    fn the_advisory_lock_hashes_the_bound_name() {
        let (sql, values) = {
            use sea_query_sqlx::SqlxBinder;
            advisory_lock_query("acme").build_sqlx(PostgresQueryBuilder)
        };
        assert!(sql.contains("pg_advisory_xact_lock(hashtext("), "{sql}");
        assert_eq!(values.0.0.len(), 1, "the name is bound, not interpolated: {sql}");
    }

    #[test]
    fn the_default_upsert_conflicts_on_db_name_and_refreshes_the_timestamp() {
        let sql = default_tenant_upsert("andromeda", "active").expect("arity").to_string(PostgresQueryBuilder);
        assert!(sql.contains("INSERT INTO \"tenants\""), "{sql}");
        assert!(sql.contains("ON CONFLICT (\"db_name\") DO UPDATE"), "{sql}");
        assert!(sql.contains("\"modified_at\" = CURRENT_TIMESTAMP"), "{sql}");
    }

    #[test]
    fn a_named_tenant_insert_is_not_the_default() {
        let sql = tenant_insert("acme", "andromeda_acme", "active")
            .expect("arity")
            .to_string(PostgresQueryBuilder);
        assert!(sql.contains("INSERT INTO \"tenants\""), "{sql}");
        assert!(!sql.contains("ON CONFLICT"), "a named tenant insert has no upsert: {sql}");
    }
}
