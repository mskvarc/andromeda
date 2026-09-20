//! Reaching, creating and migrating the database behind one tenant.
//!
//! Every pool the broker opens comes from here so that durability settings,
//! identifier limits and migration order are decided once rather than per
//! call site.

use crate::{
    error::ContextStoreError,
    schema_contract::{
        contract::current_tenant_schema_contract,
        error::SchemaContractError,
        schema_metadata::{ensure_schema_contract, verify_schema_contract},
        server_requirements::ExtensionPresence,
    },
    schema_ddl::{database::create_database, database_settings::set_database_synchronous_commit},
};
use andromeda_ngsi_ld_lexicon::tenant::TenantName;
use andromeda_postgresql_pool::configure_pool_options;
use andromeda_tenancy::{database_name::TenantDatabaseName, db_name_prefix::DbNamePrefix};
use sea_query::{Alias, Expr, ExprTrait, PostgresQueryBuilder, Query};
use sea_query_sqlx::SqlxBinder;
use sqlx::{Connection, Executor, PgConnection, PgPool, error::DatabaseError};
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
    db_name: &TenantDatabaseName,
    synchronous_commit_setting: &str,
) -> Result<(), ContextStoreError> {
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
        create_database_with_retry(&mut conn, &sql).await?;
        // `ALTER DATABASE ... SET` records the durability default on the new
        // database. It runs from the admin connection, targeting the database by
        // name, and takes effect for every session opened against it afterwards.
        let alter = set_database_synchronous_commit(db_name, synchronous_commit_setting);
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
/// exhaustion (53300) is likewise transient. A fresh broker that materialises a
/// tenant database per `--isolate-tenants` suite meets this contention in bursts,
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
fn database_exists_statement(db_name: &str) -> sea_query::SelectStatement {
    Query::select()
        .expr(Expr::val(1))
        .from(Alias::new("pg_database"))
        .and_where(Expr::col(Alias::new("datname")).eq(db_name))
        .to_owned()
}

/// Set the platform database's durability, then run tenant and platform
/// migrations against the given pool.
///
/// The pool must reach PostgreSQL directly, never through a transaction-pooling
/// proxy: the migrator holds a session advisory lock across the run, which a
/// per-transaction backend assignment would break.
///
/// The platform database is also the unnamed default tenant's database, so it
/// carries the tenant schema and its contract before the platform tables are
/// added on top. `synchronous_commit_setting` is installed on it with
/// `ALTER DATABASE ... SET` so every request-path session — including one a
/// transaction-pooling proxy opens — inherits the broker's durability.
///
/// # Errors
///
/// Returns [`ContextStoreError::Database`] when the `ALTER DATABASE` fails,
/// [`ContextStoreError::Migration`] when a tenant or platform migration fails,
/// and [`ContextStoreError::SchemaContract`] when the migrated database does not
/// carry the schema, server or extensions this binary requires.
pub async fn migrate_main_db(pool: &sqlx::PgPool, platform_db_name: &str, synchronous_commit_setting: &str) -> Result<(), ContextStoreError> {
    let alter = set_database_synchronous_commit(platform_db_name, synchronous_commit_setting);
    pool.execute(sqlx::AssertSqlSafe(alter)).await?;

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

/// Connect to a single tenant database over a one-connection pool, migrate it,
/// and close the pool.
///
/// `database_url` must reach PostgreSQL directly, never a transaction-pooling
/// proxy: the migrator holds a session advisory lock across the run, which a
/// per-transaction backend assignment would break. Migration is single-threaded,
/// so the pool holds exactly one connection.
///
/// # Errors
///
/// Returns [`ContextStoreError::Database`] when the pool cannot connect, and
/// the errors [`migrate_tenant_db`] returns when the migration or the schema
/// contract check fails.
pub async fn migrate_single_tenant_db(database_url: &Url, acquire_timeout: Duration) -> Result<(), ContextStoreError> {
    let pool = configure_pool_options(1, acquire_timeout).connect(database_url.as_str()).await?;
    migrate_tenant_db(&pool).await?;
    pool.close().await;
    Ok(())
}

/// Whether a tenant's schema is the one this binary was built against, or must
/// be migrated up to it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TenantSchemaState {
    /// The schema matches the binary's contract; no migration is needed and the
    /// caller may serve over the pool it already opened.
    Current,
    /// The schema is absent or behind and must be migrated before serving.
    NeedsMigration,
}

/// Decide whether a tenant's schema is current or must be migrated, without
/// running the migrator.
///
/// Built on the read-only [`verify_schema_contract`], which issues only single
/// statements and so is safe over a transaction pooler. A schema that is simply
/// not there yet -- unstamped, a version behind, its `schema_metadata` relation
/// undefined, or its versioned extensions not yet installed -- reads as
/// [`TenantSchemaState::NeedsMigration`], so the caller runs the migrator over a
/// direct connection. Genuine drift the migrator cannot resolve -- a digest
/// mismatch at the same version, a stamp ahead of the contract, or a server or
/// installed extension below the floor -- is returned as the refusal it is,
/// rather than migrated over.
///
/// # Errors
///
/// Returns [`ContextStoreError::SchemaContract`] for the drift cases above, and
/// when the embedded migration set has no contract or the read itself fails for
/// a reason other than an undefined relation.
pub async fn tenant_schema_state(pool: &PgPool) -> Result<TenantSchemaState, ContextStoreError> {
    let contract = current_tenant_schema_contract()?;
    match verify_schema_contract(pool, &contract).await {
        Ok(()) => Ok(TenantSchemaState::Current),
        Err(error) if schema_error_is_migratable(&error) => Ok(TenantSchemaState::NeedsMigration),
        Err(error) => Err(error.into()),
    }
}

/// Whether a schema-contract refusal is one the migrator resolves by bringing
/// the schema up to the binary's contract.
///
/// True for a schema that is not there yet; false for drift the migrator cannot
/// fix, which stays a refusal. See [`tenant_schema_state`].
fn schema_error_is_migratable(error: &SchemaContractError) -> bool {
    match error {
        SchemaContractError::Unstamped { .. } => true,
        SchemaContractError::SchemaVersionMismatch { expected, found } => found < expected,
        SchemaContractError::Database(source) => code_is_undefined_relation(source.as_database_error().and_then(DatabaseError::code).as_deref()),
        SchemaContractError::PostGisBelowMinimum { found, .. } | SchemaContractError::TimescaleDbBelowMinimum { found, .. } => {
            matches!(found, ExtensionPresence::Absent)
        }
        SchemaContractError::MigrationsDigestMismatch { .. }
        | SchemaContractError::PostgresBelowMinimum { .. }
        | SchemaContractError::NoMigrations
        | SchemaContractError::SchemaVersionValue(_)
        | SchemaContractError::MigrationsDigestValue(_)
        | SchemaContractError::ExtensionVersionValue(_) => false,
    }
}

/// Whether a SQLSTATE names an undefined relation (`42P01`), which is how an
/// unmigrated tenant's absent `schema_metadata` table surfaces on the read.
fn code_is_undefined_relation(code: Option<&str>) -> bool {
    code == Some("42P01")
}

#[cfg(test)]
mod tests {
    use crate::{
        schema_contract::{
            error::SchemaContractError,
            migrations_digest::MigrationsDigest,
            schema_version::SchemaVersion,
            server_requirements::{ExtensionPresence, ExtensionVersion, REQUIRED_POSTGIS_VERSION},
        },
        tenant_database::{DatabaseUrl, code_is_undefined_relation, database_exists_statement, schema_error_is_migratable},
    };
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

    /// The contention a burst of tenant provisioning meets is retried: 55006
    /// (template in use), 40001, 40P01 and 53300. Every other failure, and the
    /// absence of a SQLSTATE, is returned at once.
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

    /// A create that lost the race to a concurrent create of the same database
    /// name is a success, not a failure, so 42P04 (and only 42P04) is a duplicate.
    #[test]
    fn only_duplicate_database_is_treated_as_already_created() {
        use crate::tenant_database::is_duplicate_database_sqlstate;
        assert!(is_duplicate_database_sqlstate(Some("42P04")));
        assert!(!is_duplicate_database_sqlstate(Some("55006")));
        assert!(!is_duplicate_database_sqlstate(None));
    }

    /// A version behind the contract is a completed-migration-away, so the cold
    /// path migrates; a version ahead is a downgrade the migrator cannot make,
    /// so it stays a refusal.
    #[test]
    fn a_stamp_behind_the_contract_needs_migration() {
        let expected = SchemaVersion::try_from(5).expect("a valid schema version");
        let behind = SchemaVersion::try_from(4).expect("a valid schema version");
        assert!(schema_error_is_migratable(&SchemaContractError::SchemaVersionMismatch { expected, found: behind }));

        let ahead = SchemaVersion::try_from(6).expect("a valid schema version");
        assert!(!schema_error_is_migratable(&SchemaContractError::SchemaVersionMismatch { expected, found: ahead }));
    }

    /// A digest mismatch at the same version means the fleet runs two binaries,
    /// which running the migrator cannot resolve: it is refused, not migrated.
    #[test]
    fn a_digest_mismatch_at_the_same_version_is_refused_not_migrated() {
        let expected = MigrationsDigest::try_from("a".repeat(64).as_str()).expect("a valid digest");
        let found = MigrationsDigest::try_from("b".repeat(64).as_str()).expect("a valid digest");
        assert!(!schema_error_is_migratable(&SchemaContractError::MigrationsDigestMismatch { expected, found }));
    }

    /// The absent `schema_metadata` relation an unmigrated tenant carries reads
    /// back as SQLSTATE 42P01, which routes to a migration; no other SQLSTATE
    /// does.
    #[test]
    fn an_undefined_schema_metadata_relation_needs_migration() {
        assert!(code_is_undefined_relation(Some("42P01")));
        for other in [Some("42703"), Some("08006"), Some("23505"), None] {
            assert!(!code_is_undefined_relation(other), "{other:?} is not an undefined relation");
        }
    }

    /// An extension a migration installs (absent) needs migrating; one that is
    /// installed but below the floor is a server upgrade and stays a refusal.
    #[test]
    fn an_absent_extension_needs_migration_but_one_below_the_floor_is_refused() {
        let absent = SchemaContractError::PostGisBelowMinimum {
            required: REQUIRED_POSTGIS_VERSION,
            found: ExtensionPresence::Absent,
        };
        assert!(schema_error_is_migratable(&absent), "a migration installs an absent extension");

        let too_old = SchemaContractError::PostGisBelowMinimum {
            required: REQUIRED_POSTGIS_VERSION,
            found: ExtensionPresence::Installed(ExtensionVersion::new(2, 5)),
        };
        assert!(!schema_error_is_migratable(&too_old), "an installed-but-too-old extension is a server upgrade");
    }
}
