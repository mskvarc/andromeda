//! Reaching, creating and migrating the database behind one tenant.
//!
//! Every pool the broker opens comes from here so that durability settings,
//! identifier limits and migration order are decided once rather than per
//! call site.

use crate::{
    error::ContextStoreError,
    schema_contract::{contract::current_tenant_schema_contract, schema_metadata::ensure_schema_contract},
    schema_ddl::{database::create_database, session::set_synchronous_commit},
};
use andromeda_ngsi_ld_lexicon::tenant::TenantName;
use andromeda_tenancy::{database_name::TenantDatabaseName, db_name_prefix::DbNamePrefix};
use sea_query::{Alias, Expr, ExprTrait, PostgresQueryBuilder, Query};
use sea_query_sqlx::SqlxBinder;
use sqlx::{Connection, Executor, PgConnection, postgres::PgPoolOptions};
use std::{
    hash::{DefaultHasher, Hash, Hasher},
    time::Duration,
};
use url::Url;

/// How long an idle connection is kept before the pool closes it. Tenant
/// pools outlive their bursts by design (the runtime cache holds them for
/// minutes), so without this every tenant a broker ever touched would pin
/// its full pool against the server's `max_connections`.
const POOL_IDLE_TIMEOUT: Duration = Duration::from_secs(30);

/// Build a [`PgPoolOptions`] tuned with the broker's connection-level
/// settings: a per-connection `SET synchronous_commit = '<mode>'`
/// applied via `after_connect`.
///
/// Centralised so every pool the broker opens (main, per-tenant,
/// migration helpers) shares identical durability semantics.
///
/// `synchronous_commit_setting` is the PostgreSQL setting value (e.g.
/// `"on"`, `"off"`, `"local"`). The binary maps its config enum via
/// `SynchronousCommit::as_postgres_setting()` before calling.
#[must_use]
pub fn configure_pool_options(max_connections: u32, synchronous_commit_setting: &'static str) -> PgPoolOptions {
    PgPoolOptions::new()
        .min_connections(0)
        .max_connections(max_connections)
        .idle_timeout(POOL_IDLE_TIMEOUT)
        .after_connect(move |conn, _meta| {
            Box::pin(async move {
                let stmt = set_synchronous_commit(synchronous_commit_setting);
                conn.execute(sqlx::AssertSqlSafe(stmt)).await?;
                Ok(())
            })
        })
}

/// Parsed database URL wrapper used by tenancy helpers.
#[derive(Debug, Clone)]
pub struct DatabaseUrl(Url);

impl DatabaseUrl {
    /// Wrap a validated database URL.
    #[must_use]
    pub const fn new(url: Url) -> DatabaseUrl {
        DatabaseUrl(url)
    }

    /// Return the database name from the URL path.
    ///
    /// # Errors
    ///
    /// Returns [`ContextStoreError::Internal`] when the URL carries no path
    /// segment, so it names no database.
    pub fn db_name(&self) -> Result<TenantDatabaseName, ContextStoreError> {
        let path = self.0.path().trim_start_matches('/');
        if path.is_empty() {
            return Err(ContextStoreError::Internal("Database URL must include database name".to_string()));
        }
        Ok(TenantDatabaseName::from(path.to_string()))
    }

    /// Build a full URL with a replaced database name.
    #[must_use]
    pub fn with_db_name(&self, db_name: &TenantDatabaseName) -> Url {
        // clone: `Url::set_path` mutates, and the base URL is reused per tenant
        let mut url = self.0.clone();
        url.set_path(&format!("/{db_name}"));
        url
    }
}

/// Replace non-alphanumeric characters with underscores, lowercase, and
/// collapse consecutive underscores.
#[must_use]
pub fn sanitize_identifier(input: &str) -> String {
    let replaced: String = input
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch.to_ascii_lowercase() } else { '_' })
        .collect();
    let collapsed = replaced.split('_').filter(|s| !s.is_empty()).collect::<Vec<_>>().join("_");
    if collapsed.is_empty() { "tenant".to_string() } else { collapsed }
}

/// Postgres identifier length limit (NAMEDATALEN - 1 in a default build).
const POSTGRES_IDENT_MAX: usize = 63;

/// Build a per-tenant Postgres database name of the form
/// `{prefix}_{sanitized}_{hash}` while ensuring the result fits within
/// Postgres's identifier length limit. Uniqueness is preserved by the
/// fixed-length hash suffix even when the sanitized label is truncated.
#[must_use]
pub fn generate_db_name(prefix: &DbNamePrefix, tenant_name: &TenantName) -> TenantDatabaseName {
    let prefix = prefix.as_str();
    let sanitized = sanitize_identifier(tenant_name.as_str());
    let mut hasher = DefaultHasher::new();
    tenant_name.hash(&mut hasher);
    let hash = hasher.finish();
    let short_hash = format!("{:08x}", (hash & 0xffff_ffff) as u32);
    let fixed_len = prefix.len() + 1 + 1 + short_hash.len();
    let max_sanitized = POSTGRES_IDENT_MAX.saturating_sub(fixed_len);
    let truncated: String = sanitized.chars().take(max_sanitized).collect();
    let trimmed = truncated.trim_end_matches('_');
    TenantDatabaseName::from(format!("{prefix}_{trimmed}_{short_hash}"))
}

/// Create the named database if it does not already exist.
///
/// No locale or collation is specified, so the database inherits the cluster's.
/// That is deliberate: every column whose comparison decides NGSI-LD behaviour
/// declares `COLLATE "C"` itself, so the database default governs nothing the
/// specification defines, and pinning it here would hide an omitted column
/// declaration on databases Andromeda creates while leaving it live on every
/// database an operator, a managed service or a restore provisions.
/// `docs/text-collation-invariant.md` states the invariant and its inventory.
///
/// # Errors
///
/// Returns [`ContextStoreError::Database`] when the administrative connection
/// cannot be opened, when the existence probe or the `CREATE DATABASE` fails,
/// or when the connection cannot be closed cleanly.
pub async fn create_database_if_missing(base_url: &DatabaseUrl, db_name: &TenantDatabaseName) -> Result<(), ContextStoreError> {
    let admin_database = TenantDatabaseName::from("postgres".to_owned());
    let admin_url = base_url.with_db_name(&admin_database);
    let db_name = db_name.as_str();
    let mut conn = PgConnection::connect(admin_url.as_str()).await?;
    let (sql, values) = database_exists_statement(db_name).build_sqlx(PostgresQueryBuilder);
    let exists = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values)
        .fetch_optional(&mut conn)
        .await?
        .is_some();
    if !exists {
        let sql = create_database(db_name);
        conn.execute(sqlx::AssertSqlSafe(sql)).await?;
    }
    conn.close().await?;
    Ok(())
}

/// The `SELECT 1 FROM pg_database WHERE datname = $1` existence probe.
///
/// `pg_database` is a PostgreSQL system catalog, not part of the application
/// schema, so it is named by aliases; the database name is a bound value.
fn database_exists_statement(db_name: &str) -> sea_query::SelectStatement {
    Query::select()
        .expr(Expr::val(1))
        .from(Alias::new("pg_database"))
        .and_where(Expr::col(Alias::new("datname")).eq(db_name))
        .to_owned()
}

/// Run tenant and platform migrations against the given pool.
///
/// The platform database is also the unnamed default tenant's database, so it
/// carries the tenant schema and its contract before the platform tables are
/// added on top.
///
/// # Errors
///
/// Returns [`ContextStoreError::Migration`] when a tenant or platform migration
/// fails, and [`ContextStoreError::SchemaContract`] when the migrated database
/// does not carry the schema, server or extensions this binary requires.
pub async fn migrate_main_db(pool: &sqlx::PgPool) -> Result<(), ContextStoreError> {
    migrate_tenant_db(pool).await?;

    let mut platform_migrator = sqlx::migrate!("../../migrations/platform");
    platform_migrator.set_ignore_missing(true);
    platform_migrator.run(pool).await?;
    Ok(())
}

/// Run tenant-only migrations against the given pool, then stamp and verify the
/// schema contract the migrated database now carries.
///
/// The stamp is written here rather than by a migration because a migration
/// cannot digest the migration set it belongs to. Verification refuses a tenant
/// whose schema, server or extensions are not what this binary was built
/// against, before any caller can publish the pool — except a tenant whose
/// stamp is behind this binary's contract and whose applied migration history
/// proves it, which [`ensure_schema_contract`] re-stamps in place: a fleet
/// upgrade migrates every tenant's schema on this same call, and a stamp left
/// at the previous binary's version must not then read back as drift.
///
/// # Errors
///
/// Returns [`ContextStoreError::Migration`] when a tenant migration fails, and
/// [`ContextStoreError::SchemaContract`] when the embedded migration set has no
/// contract or the migrated database does not carry the schema, server or
/// extensions this binary requires.
pub async fn migrate_tenant_db(pool: &sqlx::PgPool) -> Result<(), ContextStoreError> {
    let mut tenant_migrator = sqlx::migrate!("../../migrations/tenant");
    tenant_migrator.set_ignore_missing(true);
    tenant_migrator.run(pool).await?;

    let contract = current_tenant_schema_contract()?;
    ensure_schema_contract(pool, &contract).await?;
    Ok(())
}

/// Connect to a single tenant database, migrate it, and close the pool.
///
/// # Errors
///
/// Returns [`ContextStoreError::Database`] when the pool cannot connect, and
/// the errors [`migrate_tenant_db`] returns when the migration or the schema
/// contract check fails.
pub async fn migrate_single_tenant_db(
    database_url: &Url,
    tenant_max_connections: u32,
    synchronous_commit_setting: &'static str,
) -> Result<(), ContextStoreError> {
    let pool = configure_pool_options(tenant_max_connections, synchronous_commit_setting)
        .connect(database_url.as_str())
        .await?;
    migrate_tenant_db(&pool).await?;
    pool.close().await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::tenant_database::{DatabaseUrl, POOL_IDLE_TIMEOUT, configure_pool_options, database_exists_statement};
    use andromeda_tenancy::database_name::TenantDatabaseName;
    use sea_query::PostgresQueryBuilder;

    #[test]
    fn pool_options_reclaim_idle_connections_and_keep_none_warm() {
        let options = configure_pool_options(7, "off");
        assert_eq!(options.get_max_connections(), 7);
        assert_eq!(options.get_min_connections(), 0);
        assert_eq!(options.get_idle_timeout(), Some(POOL_IDLE_TIMEOUT));
    }

    #[test]
    fn the_existence_probe_reads_the_catalog_by_bound_name() {
        let (sql, values) = database_exists_statement("andromeda_acme").build(PostgresQueryBuilder);
        assert!(sql.contains(r#"FROM "pg_database""#), "{sql}");
        assert!(sql.contains(r#""datname" = $2"#), "{sql}");
        // The constant `1` and the database name are both bound values.
        assert_eq!(values.0.len(), 2, "the name is bound, not interpolated: {sql}");
    }

    #[test]
    fn database_url_newtype_roundtrip() {
        let raw = "postgres://postgres:postgres@localhost:5432/postgres";
        let parsed = DatabaseUrl::new(raw.parse().expect("database URL"));

        let db_name = match parsed.db_name() {
            Ok(value) => value,
            Err(err) => panic!("unexpected db name error: {err}"),
        };
        pretty_assertions::assert_eq!(db_name.as_str(), "postgres");

        let rebuilt = parsed.with_db_name(&TenantDatabaseName::from("tenant_db".to_owned()));
        assert!(rebuilt.as_str().ends_with("/tenant_db"));
    }

    #[test]
    fn database_url_rejects_missing_db() {
        let raw = "postgres://postgres:postgres@localhost:5432";
        let parsed = DatabaseUrl::new(raw.parse().expect("database URL"));
        assert!(parsed.db_name().is_err());
    }
}
