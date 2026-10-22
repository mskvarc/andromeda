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
use andromeda_postgresql_seaquery::{
    aggregate::count_all,
    command::Command,
    database_definition::{ConnectedSessions, DatabaseDefinition},
    literal::true_literal,
    maintenance::to_regclass,
    statement::Statement,
};
use andromeda_tenancy::database_name::TenantDatabaseName;
use andromeda_test_harness::{configuration::TestRunConfiguration, database_access::connect_test_database};
use pretty_assertions::assert_eq;
use sea_query::{Alias, Expr, ExprTrait, Query};
use sqlx::{Connection, PgConnection, PgPool, postgres::PgPoolOptions};

/// The scratch database the refusal checks migrate and then drop.
const LEDGER_DATABASE: &str = "andromeda_verification_ledgers";

/// How many rows one ledger holds in the connected database.
async fn ledger_rows(pool: &PgPool, ledger: &str) -> i64 {
    Query::select()
        .expr(count_all())
        .from(Alias::new(ledger))
        .to_query_scalar()
        .fetch_one(pool)
        .await
        .expect("the ledger must be readable")
}

/// Drop the scratch database, whatever a previous run left behind.
async fn drop_ledger_database(base: &DatabaseUrl) {
    let admin = base.with_database_name(&TenantDatabaseName::from("postgres".to_owned()));
    let mut connection = PgConnection::connect(admin.as_str()).await.expect("administrative connection");
    DatabaseDefinition::drop(Alias::new(LEDGER_DATABASE), ConnectedSessions::Terminate)
        .to_query()
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

    let shared: bool = Query::select()
        .expr(to_regclass(Expr::val("public._sqlx_migrations")).is_not_null())
        .to_query_scalar()
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
    Query::delete()
        .from_table(Alias::new("_andromeda_platform_migrations"))
        .and_where(Expr::col(Alias::new("version")).eq(last_platform))
        .to_query()
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
    Query::insert()
        .into_table(Alias::new("_andromeda_tenant_migrations"))
        .columns(["version", "description", "success", "checksum", "execution_time"].map(Alias::new))
        .values([
            Expr::val(9999_i64),
            Expr::val("unknown"),
            true_literal(),
            Expr::val(vec![0_u8]),
            Expr::val(0_i64),
        ])
        .expect("one value per column")
        .to_query()
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
