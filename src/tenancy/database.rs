use crate::core::{config::SynchronousCommit, error::AndromedaError};
use andromeda_ngsi_ld_lexicon::tenant::TenantName;
use sqlx::{Connection, Executor, PgConnection, postgres::PgPoolOptions};
use std::hash::{Hash, Hasher};
use url::Url;

/// Build a [`PgPoolOptions`] tuned with the broker's connection-level
/// settings: a per-connection `SET synchronous_commit = '<mode>'`
/// applied via `after_connect`.
///
/// Centralised so every pool the broker opens (main, per-tenant,
/// migration helpers) shares identical durability semantics.
pub(crate) fn configure_pool_options(max_connections: u32, synchronous_commit: SynchronousCommit) -> PgPoolOptions {
    PgPoolOptions::new().max_connections(max_connections).after_connect(move |conn, _meta| {
        Box::pin(async move {
            let stmt = format!("SET synchronous_commit = '{}'", synchronous_commit.as_postgres_setting());
            conn.execute(stmt.as_str()).await?;
            Ok(())
        })
    })
}

/// Parsed database URL wrapper used by tenancy helpers.
#[derive(Debug, Clone)]
pub(crate) struct DatabaseUrl(Url);

impl DatabaseUrl {
    /// Parse and validate a Postgres database URL.
    pub(crate) fn parse(url: &str) -> Result<DatabaseUrl, AndromedaError> {
        let parsed = Url::parse(url).map_err(|e| AndromedaError::internal(format!("Invalid database URL: {e}")))?;
        Ok(DatabaseUrl(parsed))
    }

    /// Return the database name from the URL path.
    pub(crate) fn db_name(&self) -> Result<String, AndromedaError> {
        let path = self.0.path().trim_start_matches('/');
        if path.is_empty() {
            return Err(AndromedaError::internal("Database URL must include database name"));
        }
        Ok(path.to_string())
    }

    /// Build a full URL string with a replaced database name.
    pub(crate) fn with_db_name(&self, db_name: &str) -> String {
        let mut url = self.0.clone();
        url.set_path(&format!("/{db_name}"));
        url.to_string()
    }
}

pub(crate) fn sanitize_identifier(input: &str) -> String {
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
pub(crate) fn generate_db_name(prefix: &str, tenant_name: &TenantName) -> String {
    let sanitized = sanitize_identifier(tenant_name.as_str());
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    tenant_name.hash(&mut hasher);
    let hash = hasher.finish();
    let short_hash = format!("{:08x}", (hash & 0xffff_ffff) as u32);
    let fixed_len = prefix.len() + 1 + 1 + short_hash.len();
    let max_sanitized = POSTGRES_IDENT_MAX.saturating_sub(fixed_len);
    let truncated: String = sanitized.chars().take(max_sanitized).collect();
    let trimmed = truncated.trim_end_matches('_');
    format!("{prefix}_{trimmed}_{short_hash}")
}

pub(crate) async fn create_database_if_missing(base_url: &DatabaseUrl, db_name: &str) -> Result<(), AndromedaError> {
    let admin_url = base_url.with_db_name("postgres");
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

pub(crate) async fn migrate_main_db(pool: &sqlx::PgPool) -> Result<(), AndromedaError> {
    let mut tenant_migrator = sqlx::migrate!("./migrations/tenant");
    tenant_migrator.set_ignore_missing(true);
    tenant_migrator.run(pool).await?;

    let mut platform_migrator = sqlx::migrate!("./migrations/platform");
    platform_migrator.set_ignore_missing(true);
    platform_migrator.run(pool).await?;
    Ok(())
}

pub(crate) async fn migrate_tenant_db(pool: &sqlx::PgPool) -> Result<(), AndromedaError> {
    let mut tenant_migrator = sqlx::migrate!("./migrations/tenant");
    tenant_migrator.set_ignore_missing(true);
    tenant_migrator.run(pool).await?;
    Ok(())
}

pub(crate) async fn migrate_single_tenant_db(
    database_url: &str,
    tenant_max_connections: u32,
    synchronous_commit: SynchronousCommit,
) -> Result<(), AndromedaError> {
    let pool = configure_pool_options(tenant_max_connections, synchronous_commit).connect(database_url).await?;
    migrate_tenant_db(&pool).await?;
    pool.close().await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::tenancy::database::DatabaseUrl;

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
        pretty_assertions::assert_eq!(db_name, "postgres");

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
