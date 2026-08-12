//! Verification matrix row "Schema contract drift".
//!
//! Database-per-tenant means the tenant schema exists in as many copies as
//! there are tenants and every copy can drift. `schema_metadata` is the stamp
//! the broker writes once the embedded migrator has finished and re-reads
//! before the tenant's pool is published, so drift is refused there rather than
//! discovered by the first failing query.
//!
//! Every drifted state below is produced by rewriting the stamp, checked, and
//! restored before the assertion runs: a test that failed while the tenant was
//! left drifted would refuse the broker its next start.

use andromeda_postgresql_context_store::schema_contract::{contract::SchemaContract, error::SchemaContractError, schema_metadata::verify_schema_contract};
use andromeda_test_harness::{configuration::TestRunConfiguration, database_access::connect_test_database, serialized_environment::SerializedEnvironment};
use pretty_assertions::assert_eq;
use sqlx::{PgPool, Row, migrate::Migrator};

/// The migration set this test binary embeds, which is the one the broker
/// embeds: both are compiled from `migrations/tenant`.
static TENANT_MIGRATOR: Migrator = sqlx::migrate!("migrations/tenant");

/// The contract the embedded migration set expresses.
fn embedded_contract() -> SchemaContract {
    SchemaContract::of(&TENANT_MIGRATOR).expect("the embedded tenant migration set expresses a contract")
}

/// Rewrite the stamp to a drifted state, verify, and put the stamp back.
///
/// The result is returned rather than asserted on inside, so the restore
/// happens whatever the verification said.
async fn under_drift(pool: &PgPool, drift: &str) -> Result<(), SchemaContractError> {
    let stamped = sqlx::query("SELECT schema_version, migrations_sha256 FROM schema_metadata WHERE singleton")
        .fetch_optional(pool)
        .await
        .expect("the stamp must be readable")
        .map(|row| (row.get::<i32, _>("schema_version"), row.get::<String, _>("migrations_sha256")));

    sqlx::query(drift).execute(pool).await.expect("the drift statement must apply");
    let verified = verify_schema_contract(pool, &embedded_contract()).await;

    sqlx::query("DELETE FROM schema_metadata")
        .execute(pool)
        .await
        .expect("the stamp must be removable");
    if let Some((version, digest)) = stamped {
        sqlx::query("INSERT INTO schema_metadata (applied_at, schema_version, singleton, migrations_sha256) VALUES (NOW(), $1, TRUE, $2)")
            .bind(version)
            .bind(digest)
            .execute(pool)
            .await
            .expect("the original stamp must be restorable");
    }
    verified
}

#[tokio::test]
async fn a_tenant_stamped_by_this_binary_is_accepted_and_reports_the_version_it_applied() {
    let pool = connect_test_database().await.expect("test database connection");
    let contract = embedded_contract();

    let row = sqlx::query("SELECT schema_version, migrations_sha256 FROM schema_metadata WHERE singleton")
        .fetch_one(&pool)
        .await
        .expect("a migrated tenant carries a stamp");
    assert_eq!(
        row.get::<i32, _>("schema_version"),
        contract.version().get(),
        "the tenant is not at the version this binary embeds"
    );
    assert_eq!(
        row.get::<String, _>("migrations_sha256"),
        contract.digest().to_string(),
        "the tenant was migrated along a different path"
    );

    verify_schema_contract(&pool, &contract)
        .await
        .expect("a tenant stamped by this binary must serve normally");

    // The platform catalogue names every tenant; the default one is the
    // database this suite runs against, and its applied version is the stamp
    // above.
    let registered: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM tenants")
        .fetch_one(&pool)
        .await
        .expect("the platform tenant catalogue must be readable");
    assert!(registered > 0, "the platform catalogue must name at least the default tenant");

    pool.close().await;
}

#[tokio::test]
async fn a_tenant_stamped_with_another_schema_version_is_refused_naming_both_versions() {
    let configuration = TestRunConfiguration::from_environment().expect("integration test configuration");
    let environment = SerializedEnvironment::acquire(&configuration).await.expect("exclusive ownership of the stamp");
    let pool = connect_test_database().await.expect("test database connection");

    let refusal = under_drift(&pool, "UPDATE schema_metadata SET schema_version = schema_version - 1 WHERE singleton").await;

    match refusal {
        Err(SchemaContractError::SchemaVersionMismatch { expected, found }) => {
            assert_eq!(expected, embedded_contract().version(), "the refusal must name what the binary requires");
            assert_eq!(found.get(), expected.get() - 1, "the refusal must name what the tenant carries");
        }
        Err(other) => unreachable!("a version mismatch must be reported as such: {other}"),
        Ok(()) => unreachable!("a tenant one version behind must be refused"),
    }

    pool.close().await;
    environment.finish().await.expect("releasing exclusive ownership failed");
}

#[tokio::test]
async fn a_tenant_stamped_with_another_migration_set_is_refused_naming_both_digests() {
    let configuration = TestRunConfiguration::from_environment().expect("integration test configuration");
    let environment = SerializedEnvironment::acquire(&configuration).await.expect("exclusive ownership of the stamp");
    let pool = connect_test_database().await.expect("test database connection");

    // A digest of the right shape that no migration set produces: the column's
    // check constraint accepts it, so the refusal comes from the contract check
    // rather than from the database.
    let refusal = under_drift(&pool, "UPDATE schema_metadata SET migrations_sha256 = repeat('a', 64) WHERE singleton").await;

    match refusal {
        Err(SchemaContractError::MigrationsDigestMismatch { expected, found }) => {
            assert_eq!(expected, embedded_contract().digest(), "the refusal must name the digest the binary requires");
            assert_eq!(found.to_string(), "a".repeat(64), "the refusal must name the digest the tenant carries");
        }
        Err(other) => unreachable!("a digest mismatch must be reported as such: {other}"),
        Ok(()) => unreachable!("a tenant migrated along another path must be refused"),
    }

    pool.close().await;
    environment.finish().await.expect("releasing exclusive ownership failed");
}

/// A tenant restored from a backup taken before the stamp existed is migrated
/// but unstamped, which is drift rather than a state to repair silently.
#[tokio::test]
async fn a_migrated_but_unstamped_tenant_is_refused_naming_what_it_should_carry() {
    let configuration = TestRunConfiguration::from_environment().expect("integration test configuration");
    let environment = SerializedEnvironment::acquire(&configuration).await.expect("exclusive ownership of the stamp");
    let pool = connect_test_database().await.expect("test database connection");

    let refusal = under_drift(&pool, "DELETE FROM schema_metadata").await;

    match refusal {
        Err(SchemaContractError::Unstamped {
            expected_version,
            expected_digest,
        }) => {
            let contract = embedded_contract();
            assert_eq!(expected_version, contract.version(), "the refusal must name the version to stamp");
            assert_eq!(expected_digest, contract.digest(), "the refusal must name the digest to stamp");
        }
        Err(other) => unreachable!("an unstamped tenant must be reported as such: {other}"),
        Ok(()) => unreachable!("an unstamped tenant must be refused"),
    }

    pool.close().await;
    environment.finish().await.expect("releasing exclusive ownership failed");
}

/// The server and extension floors are part of the same check, so a tenant is
/// only accepted when its server can actually run the schema.
#[tokio::test]
async fn the_server_and_extensions_meet_the_floors_the_schema_depends_on() {
    let pool = connect_test_database().await.expect("test database connection");

    let row = sqlx::query(
        "SELECT current_setting('server_version_num')::INTEGER AS server, \
            (SELECT extversion FROM pg_extension WHERE extname = 'postgis') AS postgis, \
            (SELECT extversion FROM pg_extension WHERE extname = 'timescaledb') AS timescaledb",
    )
    .fetch_one(&pool)
    .await
    .expect("the server and extension versions must be readable");

    let server: i32 = row.get("server");
    assert!(server >= 150_000, "NULLS NOT DISTINCT needs PostgreSQL 15, server reports {server}");
    assert!(
        row.get::<Option<String>, _>("postgis").is_some(),
        "the schema stores geometries and needs PostGIS"
    );
    assert!(
        row.get::<Option<String>, _>("timescaledb").is_some(),
        "the history relations are hypertables and need TimescaleDB"
    );

    pool.close().await;
}
