//! Reaching and creating the database behind one tenant.
//!
//! Every database the broker creates comes from here so that durability
//! settings and identifier limits are decided once rather than per call site.
//! Migrating a database is [`crate::tenant_migration`]'s concern.

use crate::{
    error::ContextStoreError,
    schema_ddl::{database::create_database, database_settings::set_database_synchronous_commit},
};
use andromeda_ngsi_ld_lexicon::tenant::TenantName;
use andromeda_tenancy::{database_name::TenantDatabaseName, db_name_prefix::DbNamePrefix};
use sea_query::{Alias, Expr, ExprTrait, PostgresQueryBuilder, Query};
use sea_query_sqlx::SqlxBinder;
use sqlx::{Connection, Executor, PgConnection, error::DatabaseError};
use std::{
    hash::{DefaultHasher, Hash, Hasher},
    time::Duration,
};
use tokio::time::sleep;
use url::Url;

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
    /// Returns [`ContextStoreError::DatabaseUrlUnnamed`] when the URL carries no
    /// path segment, so it names no database.
    pub fn database_name(&self) -> Result<TenantDatabaseName, ContextStoreError> {
        let path = self.0.path().trim_start_matches('/');
        if path.is_empty() {
            return Err(ContextStoreError::DatabaseUrlUnnamed);
        }
        Ok(TenantDatabaseName::from(path.to_string()))
    }

    /// Build a full URL with a replaced database name.
    #[must_use]
    pub fn with_database_name(&self, database_name: &TenantDatabaseName) -> Url {
        // clone: `Url::set_path` mutates, and the base URL is reused per tenant
        let mut url = self.0.clone();
        url.set_path(&format!("/{database_name}"));
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
pub fn generate_database_name(prefix: &DbNamePrefix, tenant_name: &TenantName) -> TenantDatabaseName {
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
///
/// `synchronous_commit_setting` is installed on the freshly created database
/// with `ALTER DATABASE ... SET` so every session the request path later opens
/// through a transaction-pooling proxy inherits the broker's durability without
/// a per-connection `SET`.
///
/// # Errors
///
/// Returns [`ContextStoreError::Database`] when the administrative connection
/// cannot be opened, when the existence probe, the `CREATE DATABASE` or the
/// `ALTER DATABASE` fails, or when the connection cannot be closed cleanly.
pub async fn create_database_if_missing(
    base_url: &DatabaseUrl,
    database_name: &TenantDatabaseName,
    synchronous_commit_setting: &str,
) -> Result<(), ContextStoreError> {
    let admin_database = TenantDatabaseName::from("postgres".to_owned());
    let admin_url = base_url.with_database_name(&admin_database);
    let database_name = database_name.as_str();
    let mut conn = PgConnection::connect(admin_url.as_str()).await?;
    let (sql, values) = database_exists_statement(database_name).build_sqlx(PostgresQueryBuilder);
    let exists = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values)
        .fetch_optional(&mut conn)
        .await?
        .is_some();
    if !exists {
        let sql = create_database(database_name);
        create_database_with_retry(&mut conn, &sql).await?;
        // `ALTER DATABASE ... SET` records the durability default on the new
        // database. It runs from the admin connection, targeting the database by
        // name, and takes effect for every session opened against it afterwards.
        let alter = set_database_synchronous_commit(database_name, synchronous_commit_setting);
        conn.execute(sqlx::AssertSqlSafe(alter)).await?;
    }
    conn.close().await?;
    Ok(())
}

/// The attempts a contended `CREATE DATABASE` is given before its failure is reported.
const CREATE_DATABASE_ATTEMPTS: u32 = 6;
/// The delay between contended `CREATE DATABASE` attempts.
const CREATE_DATABASE_BACKOFF: Duration = Duration::from_millis(250);

/// Execute `CREATE DATABASE`, retrying the transient contention that arises when
/// many tenants are provisioned at once.
///
/// PostgreSQL copies a template database for every `CREATE DATABASE` and rejects
/// one whose template is being read by a concurrent create with SQLSTATE 55006
/// (`object_in_use`, "source database ... is being accessed by other users"); a
/// deadlock (40P01), a serialization failure (40001) or momentary connection
/// exhaustion (53300) is likewise transient. A broker that materialises many
/// tenant databases at once meets this contention in bursts,
/// which otherwise surfaces to the client as a 500 on the first request in the
/// new tenant. A create that lost the race to a concurrent create of the same
/// name (42P04) finds its database already made, which is success.
async fn create_database_with_retry(conn: &mut PgConnection, sql: &str) -> Result<(), ContextStoreError> {
    let mut attempt = 0;
    loop {
        attempt += 1;
        match conn.execute(sqlx::AssertSqlSafe(sql)).await {
            Ok(_) => return Ok(()),
            Err(error) if is_database_already_exists(&error) => return Ok(()),
            Err(error) if attempt < CREATE_DATABASE_ATTEMPTS && is_transient_create_failure(&error) => {
                sleep(CREATE_DATABASE_BACKOFF).await;
            }
            Err(error) => return Err(error.into()),
        }
    }
}

/// Whether a `CREATE DATABASE` failure is transient contention a retry can clear.
fn is_transient_create_failure(error: &sqlx::Error) -> bool {
    is_transient_create_sqlstate(error.as_database_error().and_then(DatabaseError::code).as_deref())
}

/// Whether a `CREATE DATABASE` failed because a concurrent provisioner already
/// created the database (SQLSTATE 42P04, `duplicate_database`).
fn is_database_already_exists(error: &sqlx::Error) -> bool {
    is_duplicate_database_sqlstate(error.as_database_error().and_then(DatabaseError::code).as_deref())
}

/// Whether `code` is a transient `CREATE DATABASE` SQLSTATE: 55006 (template in
/// use by a concurrent create), 40001 (serialization), 40P01 (deadlock) or 53300
/// (too many connections).
fn is_transient_create_sqlstate(code: Option<&str>) -> bool {
    code.is_some_and(|code| matches!(code, "55006" | "40001" | "40P01" | "53300"))
}

/// Whether `code` is the `duplicate_database` SQLSTATE (42P04).
fn is_duplicate_database_sqlstate(code: Option<&str>) -> bool {
    code == Some("42P04")
}

/// The `SELECT 1 FROM pg_database WHERE datname = $1` existence probe.
///
/// `pg_database` is a PostgreSQL system catalog, not part of the application
/// schema, so it is named by aliases; the database name is a bound value.
fn database_exists_statement(database_name: &str) -> sea_query::SelectStatement {
    Query::select()
        .expr(Expr::val(1))
        .from(Alias::new("pg_database"))
        .and_where(Expr::col(Alias::new("datname")).eq(database_name))
        .to_owned()
}

#[cfg(test)]
mod tests {
    use crate::tenant_database::{DatabaseUrl, database_exists_statement};
    use andromeda_tenancy::database_name::TenantDatabaseName;
    use sea_query::PostgresQueryBuilder;

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

        let database_name = match parsed.database_name() {
            Ok(value) => value,
            Err(err) => panic!("unexpected db name error: {err}"),
        };
        pretty_assertions::assert_eq!(database_name.as_str(), "postgres");

        let rebuilt = parsed.with_database_name(&TenantDatabaseName::from("tenant_db".to_owned()));
        assert!(rebuilt.as_str().ends_with("/tenant_db"));
    }

    #[test]
    fn database_url_rejects_missing_db() {
        let raw = "postgres://postgres:postgres@localhost:5432";
        let parsed = DatabaseUrl::new(raw.parse().expect("database URL"));
        assert!(parsed.database_name().is_err());
    }

    #[test]
    fn only_the_concurrent_create_database_sqlstates_are_retried() {
        use crate::tenant_database::is_transient_create_sqlstate;
        for code in ["55006", "40001", "40P01", "53300"] {
            assert!(is_transient_create_sqlstate(Some(code)), "{code}");
        }
        for code in ["42P04", "42501", "23505", "3D000", "08006"] {
            assert!(!is_transient_create_sqlstate(Some(code)), "{code}");
        }
        assert!(!is_transient_create_sqlstate(None));
    }

    #[test]
    fn only_duplicate_database_is_treated_as_already_created() {
        use crate::tenant_database::is_duplicate_database_sqlstate;
        assert!(is_duplicate_database_sqlstate(Some("42P04")));
        assert!(!is_duplicate_database_sqlstate(Some("55006")));
        assert!(!is_duplicate_database_sqlstate(None));
    }
}
