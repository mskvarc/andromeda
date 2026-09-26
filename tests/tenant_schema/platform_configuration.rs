//! Configuring the platform database when several brokers start at once.
//!
//! Every broker installs its durability on the platform database with
//! `ALTER DATABASE ... SET` as it starts. On a database that has no setting row
//! yet, concurrent brokers race to insert it; the race must not fail a
//! broker's startup.

use andromeda_postgresql_context_store::{
    config::SynchronousCommit,
    tenant_database::{DatabaseUrl, create_database_if_missing},
    tenant_migration::configure_platform_database,
};
use andromeda_tenancy::database_name::TenantDatabaseName;
use andromeda_test_harness::configuration::TestRunConfiguration;
use futures::future::join_all;
use sqlx::{Connection, PgConnection, postgres::PgPoolOptions};

/// The scratch database the concurrent configuration targets.
const CONFIGURATION_DATABASE: &str = "andromeda_verification_platform_configuration";

/// How many brokers configure the database at once.
const CONCURRENT_BROKERS: usize = 8;

#[tokio::test]
async fn concurrent_brokers_configuring_a_fresh_platform_database_all_succeed() {
    let configuration = TestRunConfiguration::from_environment().expect("integration test configuration");
    // clone: `DatabaseUrl` owns the endpoint it derives per-database URLs from.
    let base = DatabaseUrl::new(configuration.database().url().clone());
    let name = TenantDatabaseName::from(CONFIGURATION_DATABASE.to_owned());
    create_database_if_missing(&base, &name, SynchronousCommit::Off.as_postgres_setting())
        .await
        .expect("scratch database");

    // Remove the setting row, so the database is as fresh as an operator-created one.
    let admin = base.with_database_name(&TenantDatabaseName::from("postgres".to_owned()));
    let mut connection = PgConnection::connect(admin.as_str()).await.expect("administrative connection");
    sqlx::query(sqlx::AssertSqlSafe(format!("ALTER DATABASE {CONFIGURATION_DATABASE} RESET ALL")))
        .execute(&mut connection)
        .await
        .expect("the setting row must be removable");

    let pool = PgPoolOptions::new()
        .max_connections(u32::try_from(CONCURRENT_BROKERS).expect("broker count fits"))
        .connect(base.with_database_name(&name).as_str())
        .await
        .expect("scratch database connection");
    let outcomes =
        join_all((0..CONCURRENT_BROKERS).map(|_| configure_platform_database(&pool, CONFIGURATION_DATABASE, SynchronousCommit::Off.as_postgres_setting())))
            .await;
    for outcome in outcomes {
        assert!(outcome.is_ok(), "a concurrent configuration must not fail: {outcome:?}");
    }
    pool.close().await;

    sqlx::query(sqlx::AssertSqlSafe(format!("DROP DATABASE IF EXISTS {CONFIGURATION_DATABASE} WITH (FORCE)")))
        .execute(&mut connection)
        .await
        .expect("the scratch database must be droppable");
    connection.close().await.expect("administrative connection closes");
}
