//! Reaching, creating and migrating the database behind one tenant.
//!
//! Every pool the broker opens comes from here so that durability settings,
//! identifier limits and migration order are decided once rather than per
//! call site.

use crate::error::ContextStoreError;
use andromeda_ngsi_ld_lexicon::tenant::TenantName;
use andromeda_tenancy::{database_name::TenantDatabaseName, db_name_prefix::DbNamePrefix};
use sqlx::{Connection, Executor, PgConnection, postgres::PgPoolOptions};
use std::hash::{DefaultHasher, Hash, Hasher};
use url::Url;

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
pub fn configure_pool_options(max_connections: u32, synchronous_commit_setting: &'static str) -> PgPoolOptions {
    PgPoolOptions::new().max_connections(max_connections).after_connect(move |conn, _meta| {
        Box::pin(async move {
            let stmt = format!("SET synchronous_commit = '{synchronous_commit_setting}'");
            conn.execute(stmt.as_str()).await?;
            Ok(())
        })
    })
}

/// Parsed database URL wrapper used by tenancy helpers.
#[derive(Debug, Clone)]
pub struct DatabaseUrl(Url);

impl DatabaseUrl {
    /// Parse and validate a Postgres database URL.
    pub fn parse(url: &str) -> Result<DatabaseUrl, ContextStoreError> {
        let parsed = Url::parse(url).map_err(|e| ContextStoreError::Internal(format!("Invalid database URL: {e}")))?;
        Ok(DatabaseUrl(parsed))
    }

    /// Return the database name from the URL path.
    pub fn db_name(&self) -> Result<TenantDatabaseName, ContextStoreError> {
        let path = self.0.path().trim_start_matches('/');
        if path.is_empty() {
            return Err(ContextStoreError::Internal("Database URL must include database name".to_string()));
        }
        Ok(TenantDatabaseName::from(path.to_string()))
    }

    /// Build a full URL string with a replaced database name.
    pub fn with_db_name(&self, db_name: &str) -> String {
        // clone: `Url::set_path` mutates, and the base URL is reused per tenant
        let mut url = self.0.clone();
        url.set_path(&format!("/{db_name}"));
        url.to_string()
    }
}

/// Replace non-alphanumeric characters with underscores, lowercase, and
/// collapse consecutive underscores.
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
pub async fn create_database_if_missing(base_url: &DatabaseUrl, db_name: &TenantDatabaseName) -> Result<(), ContextStoreError> {
    let admin_url = base_url.with_db_name("postgres");
    let db_name = db_name.as_str();
    let mut conn = PgConnection::connect(&admin_url).await?;
    let exists = sqlx::query("SELECT 1 FROM pg_database WHERE datname = $1")
        .bind(db_name)
        .fetch_optional(&mut conn)
        .await?
        .is_some();
    if !exists {
        let sql = format!("CREATE DATABASE \"{}\"", db_name.replace('"', ""));
        conn.execute(sql.as_str()).await?;
    }
    conn.close().await?;
    Ok(())
}

/// Run tenant and platform migrations against the given pool.
pub async fn migrate_main_db(pool: &sqlx::PgPool) -> Result<(), ContextStoreError> {
    let mut tenant_migrator = sqlx::migrate!("../../migrations/tenant");
    tenant_migrator.set_ignore_missing(true);
    tenant_migrator.run(pool).await?;

    let mut platform_migrator = sqlx::migrate!("../../migrations/platform");
    platform_migrator.set_ignore_missing(true);
    platform_migrator.run(pool).await?;
    Ok(())
}

/// Run tenant-only migrations against the given pool.
pub async fn migrate_tenant_db(pool: &sqlx::PgPool) -> Result<(), ContextStoreError> {
    let mut tenant_migrator = sqlx::migrate!("../../migrations/tenant");
    tenant_migrator.set_ignore_missing(true);
    tenant_migrator.run(pool).await?;
    Ok(())
}

/// Connect to a single tenant database, migrate it, and close the pool.
pub async fn migrate_single_tenant_db(
    database_url: &str,
    tenant_max_connections: u32,
    synchronous_commit_setting: &'static str,
) -> Result<(), ContextStoreError> {
    let pool = configure_pool_options(tenant_max_connections, synchronous_commit_setting)
        .connect(database_url)
        .await?;
    migrate_tenant_db(&pool).await?;
    pool.close().await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::tenant_database::DatabaseUrl;

    #[test]
    fn database_url_newtype_roundtrip() {
        let raw = "postgres://postgres:postgres@localhost:5432/postgres";
        let parsed = match DatabaseUrl::parse(raw) {
            Ok(value) => value,
            Err(err) => unreachable!("unexpected parse error: {err}"),
        };

        let db_name = match parsed.db_name() {
            Ok(value) => value,
            Err(err) => unreachable!("unexpected db name error: {err}"),
        };
        pretty_assertions::assert_eq!(db_name.as_str(), "postgres");

        let rebuilt = parsed.with_db_name("tenant_db");
        assert!(rebuilt.ends_with("/tenant_db"));
    }

    #[test]
    fn database_url_rejects_missing_db() {
        let raw = "postgres://postgres:postgres@localhost:5432";
        let parsed = match DatabaseUrl::parse(raw) {
            Ok(value) => value,
            Err(err) => unreachable!("unexpected parse error: {err}"),
        };
        assert!(parsed.db_name().is_err());
    }
}
