//! The three migration ledgers and what each refuses.
//!
//! Platform, tenant and branch migrations are recorded in their own ledgers,
//! none of them shared and none of them
//! tolerant of a history the embedded set does not account for. The platform
//! database carries both the platform and the tenant ledger and never the
//! sqlx default, and a scratch database shows that a ledger row the binary
//! cannot account for -- one missing, one unknown -- fails the migration
//! rather than being skipped.

use andromeda_postgresql_context_store::{
    migration_set::{platform::platform_migrations, tenant::tenant_migrator},
    tenant_database::{DatabaseUrl, create_database_if_missing},
    tenant_migration::migrate_platform_db,
};
use andromeda_tenancy::database_name::TenantDatabaseName;
use andromeda_test_harness::{configuration::TestRunConfiguration, database_access::connect_test_database};
use pretty_assertions::assert_eq;
use sqlx::{Connection, PgConnection, PgPool, postgres::PgPoolOptions};

/// The scratch database the refusal checks migrate and then drop.
const LEDGER_DATABASE: &str = "andromeda_verification_ledgers";

/// How many rows one ledger holds in the connected database.
async fn ledger_rows(pool: &PgPool, ledger: &str) -> i64 {
    // `ledger` is one of this module's fixed ledger names, never caller input.
    sqlx::query_scalar(sqlx::AssertSqlSafe(format!("SELECT COUNT(*) FROM {ledger}")))
        .fetch_one(pool)
        .await
        .expect("the ledger must be readable")
}

/// Drop the scratch database, whatever a previous run left behind.
async fn drop_ledger_database(base: &DatabaseUrl) {
    let admin = base.with_database_name(&TenantDatabaseName::from("postgres".to_owned()));
    let mut connection = PgConnection::connect(admin.as_str()).await.expect("administrative connection");
    sqlx::query(sqlx::AssertSqlSafe(format!("DROP DATABASE IF EXISTS {LEDGER_DATABASE} WITH (FORCE)")))
        .execute(&mut connection)
        .await
        .expect("the scratch database must be droppable");
    connection.close().await.expect("administrative connection closes");
}

/// A freshly created and fully migrated scratch platform database.
async fn fresh_platform_database(base: &DatabaseUrl) -> PgPool {
    drop_ledger_database(base).await;
    let name = TenantDatabaseName::from(LEDGER_DATABASE.to_owned());
    create_database_if_missing(base, &name).await.expect("scratch database");
    let pool = PgPoolOptions::new()
        .max_connections(2)
        .connect(base.with_database_name(&name).as_str())
        .await
        .expect("scratch database connection");
    migrate_platform_db(&pool).await.expect("a fresh platform database migrates");
    pool
}

#[tokio::test]
async fn the_platform_database_keeps_a_ledger_per_migration_set_and_no_shared_one() {
    let pool = connect_test_database().await.expect("test database connection");

    let platform = i64::try_from(platform_migrations().len()).expect("migration count fits");
    let tenant = i64::try_from(tenant_migrator().iter().count()).expect("migration count fits");
    assert_eq!(ledger_rows(&pool, "_andromeda_platform_migrations").await, platform);
    assert_eq!(ledger_rows(&pool, "_andromeda_tenant_migrations").await, tenant);

    let shared: bool = sqlx::query_scalar("SELECT to_regclass('public._sqlx_migrations') IS NOT NULL")
        .fetch_one(&pool)
        .await
        .expect("the catalogue must be readable");
    assert!(!shared, "no migration set records into the sqlx default ledger");

    pool.close().await;
}

#[tokio::test]
async fn a_ledger_the_binary_cannot_account_for_fails_the_migration() {
    let configuration = TestRunConfiguration::from_environment().expect("integration test configuration");
    // clone: `DatabaseUrl` owns the endpoint it derives per-database URLs from.
    let base = DatabaseUrl::new(configuration.database().url().clone());

    // A shipped migration whose ledger row is gone is applied again, and a
    // file that creates its relations outright cannot be applied twice.
    let pool = fresh_platform_database(&base).await;
    let last_platform = platform_migrations().last().expect("the platform set is not empty").version;
    sqlx::query("DELETE FROM _andromeda_platform_migrations WHERE version = $1")
        .bind(last_platform)
        .execute(&pool)
        .await
        .expect("the ledger row must be removable");
    assert!(
        migrate_platform_db(&pool).await.is_err(),
        "a missing ledger row must fail the migration, not be silently re-applied"
    );
    pool.close().await;

    // A ledger row no embedded migration accounts for is refused rather than
    // ignored: that history belongs to a binary this one does not know.
    let pool = fresh_platform_database(&base).await;
    sqlx::query(
        "INSERT INTO _andromeda_tenant_migrations (version, description, success, checksum, execution_time) \
         VALUES (9999, 'unknown', TRUE, '\\x00'::bytea, 0)",
    )
    .execute(&pool)
    .await
    .expect("the unknown ledger row must be insertable");
    assert!(
        migrate_platform_db(&pool).await.is_err(),
        "a ledger row the binary does not embed must fail the migration"
    );
    pool.close().await;

    drop_ledger_database(&base).await;
}
