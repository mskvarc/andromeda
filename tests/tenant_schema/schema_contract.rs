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

use andromeda_postgresql_context_store::schema_contract::{
    contract::SchemaContract,
    error::SchemaContractError,
    schema_metadata::{ensure_schema_contract, verify_schema_contract},
};
use andromeda_test_harness::{configuration::TestRunConfiguration, database_access::connect_test_database, serialized_environment::SerializedEnvironment};
use pretty_assertions::assert_eq;
use sqlx::{
    PgPool,
    Row,
    migrate::{Migration, Migrator},
};
use std::borrow::Cow;

/// The migration set this test binary embeds, which is the one the broker
/// embeds: both are compiled from `migrations/tenant`.
static TENANT_MIGRATOR: Migrator = sqlx::migrate!("migrations/tenant");

/// The contract the embedded migration set expresses.
fn embedded_contract() -> SchemaContract {
    SchemaContract::of(&TENANT_MIGRATOR).expect("the embedded tenant migration set expresses a contract")
}

/// The contract expressed by every non-down migration this binary embeds at
/// or below `cutoff_version`.
///
/// Built from clones of this binary's own resolved [`Migration`] values
/// rather than reconstructed ones, so the digest is byte-identical to what
/// `_sqlx_migrations` records for that same prefix on the database this test
/// binary actually migrated: the real prefix of a real history, not an
/// approximation of one.
fn contract_up_to(cutoff_version: i64) -> SchemaContract {
    let prefix = TENANT_MIGRATOR
        .iter()
        .filter(|migration| !migration.migration_type.is_down_migration() && migration.version <= cutoff_version)
        .cloned()
        .collect::<Vec<Migration>>();
    let migrator = Migrator {
        migrations: Cow::Owned(prefix),
        ..Migrator::DEFAULT
    };
    SchemaContract::of(&migrator).expect("a non-empty migration prefix expresses a contract")
}

/// The highest migration version below the embedded contract's own, i.e. the
/// version a tenant migrated by "the previous release" would carry.
fn previous_version() -> i64 {
    let mut versions = TENANT_MIGRATOR
        .iter()
        .filter(|migration| !migration.migration_type.is_down_migration())
        .map(|migration| migration.version)
        .collect::<Vec<i64>>();
    versions.sort_unstable();
    versions.dedup();
    let index = versions.len().checked_sub(2).expect("the embedded migration set carries at least two versions");
    versions[index]
}

/// The raw `schema_metadata` row a pool currently carries, if any.
async fn read_stamp_row(pool: &PgPool) -> Option<(i32, String)> {
    sqlx::query("SELECT schema_version, migrations_sha256 FROM schema_metadata WHERE singleton")
        .fetch_optional(pool)
        .await
        .expect("the stamp must be readable")
        .map(|row| (row.get::<i32, _>("schema_version"), row.get::<String, _>("migrations_sha256")))
}

/// Rewrite the stamp to a drifted state, verify, and put the stamp back.
///
/// The result is returned rather than asserted on inside, so the restore
/// happens whatever the verification said.
async fn under_drift(pool: &PgPool, drift: &str) -> Result<(), SchemaContractError> {
    let stamped = read_stamp_row(pool).await;

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

/// Rewrite the stamp to a drifted state, boot the tenant through the real
/// production entry point, and put the stamp back.
///
/// [`ensure_schema_contract`] is what `migrate_tenant_db` calls on every
/// tenant boot, so this is the path the review flagged: a stamp-first
/// implementation would silently overwrite the drift this induces instead of
/// refusing it. The row `ensure_schema_contract` leaves behind is captured
/// before the restore, so the caller can assert the drifted value survived the
/// call untouched rather than only that an error came back.
async fn under_drift_through_ensure(pool: &PgPool, drift: &str) -> (Result<(), SchemaContractError>, Option<(i32, String)>) {
    let stamped = read_stamp_row(pool).await;

    sqlx::query(drift).execute(pool).await.expect("the drift statement must apply");
    let outcome = ensure_schema_contract(pool, &embedded_contract()).await;
    let row_after_ensure = read_stamp_row(pool).await;

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
    (outcome, row_after_ensure)
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

/// Verification matrix row "Schema contract drift": a stamp behind the
/// contract is only ever advanced when the tenant's real applied migration
/// history proves it; a decremented version whose digest was never a real
/// prefix of that history is refused like any other drift, not restamped
/// over.
#[tokio::test]
async fn ensure_schema_contract_refuses_a_tenant_stamped_behind_with_a_digest_that_does_not_match_its_real_history_and_leaves_the_drift_untouched() {
    let configuration = TestRunConfiguration::from_environment().expect("integration test configuration");
    let environment = SerializedEnvironment::acquire(&configuration).await.expect("exclusive ownership of the stamp");
    let pool = connect_test_database().await.expect("test database connection");
    let contract = embedded_contract();

    // The version is drifted behind the contract, but the digest is left as
    // the binary's full-contract digest rather than the digest a real earlier
    // binary would have stamped at that version -- no genuine migration
    // history produces this pairing, so it must read as drift rather than as
    // a completed upgrade.
    let (refusal, row_after_ensure) = under_drift_through_ensure(&pool, "UPDATE schema_metadata SET schema_version = schema_version - 1 WHERE singleton").await;

    match refusal {
        Err(SchemaContractError::MigrationsDigestMismatch { expected, found }) => {
            assert_ne!(
                expected,
                contract.digest(),
                "the real applied-history digest at the drifted version is not the full-contract digest"
            );
            assert_eq!(found, contract.digest(), "the refusal must name the digest the drifted stamp actually carries");
        }
        Err(other) => unreachable!("an inconsistent behind-contract stamp must be reported as a digest mismatch: {other}"),
        Ok(()) => unreachable!("a stamp whose digest does not match its real history must be refused, not silently restamped"),
    }
    let (row_version, row_digest) = row_after_ensure.expect("a stamp present before the call must still be present after a refusal");
    assert_eq!(
        row_version,
        contract.version().get() - 1,
        "a refused ensure_schema_contract must not overwrite the drifted version"
    );
    assert_eq!(
        row_digest,
        contract.digest().to_string(),
        "the digest column was never drifted and must stay untouched"
    );

    pool.close().await;
    environment.finish().await.expect("releasing exclusive ownership failed");
}

/// Verification matrix row "Schema contract drift": a stamp left behind at an
/// earlier binary's version is advanced, not refused, once the tenant's real
/// applied migration history proves it was a completed upgrade -- the
/// scenario a fleet upgrade produces on every tenant it migrates.
#[tokio::test]
async fn ensure_schema_contract_advances_a_stamp_that_is_behind_but_matches_the_tenants_real_migration_history() {
    let configuration = TestRunConfiguration::from_environment().expect("integration test configuration");
    let environment = SerializedEnvironment::acquire(&configuration).await.expect("exclusive ownership of the stamp");
    let pool = connect_test_database().await.expect("test database connection");
    let contract = embedded_contract();
    let previous = contract_up_to(previous_version());
    assert!(
        previous.version() < contract.version(),
        "the fixture must name a version genuinely behind the binary's contract"
    );

    let stamped_before = read_stamp_row(&pool).await;
    sqlx::query("UPDATE schema_metadata SET schema_version = $1, migrations_sha256 = $2 WHERE singleton")
        .bind(previous.version().get())
        .bind(previous.digest().to_string())
        .execute(&pool)
        .await
        .expect("the drift statement must apply");

    let outcome = ensure_schema_contract(&pool, &contract).await;
    let row_after_ensure = read_stamp_row(&pool).await;

    sqlx::query("DELETE FROM schema_metadata")
        .execute(&pool)
        .await
        .expect("the stamp must be removable");
    if let Some((version, digest)) = stamped_before {
        sqlx::query("INSERT INTO schema_metadata (applied_at, schema_version, singleton, migrations_sha256) VALUES (NOW(), $1, TRUE, $2)")
            .bind(version)
            .bind(digest)
            .execute(&pool)
            .await
            .expect("the original stamp must be restorable");
    }

    outcome.expect("a stamp that is behind but consistent with the tenant's real migration history must be advanced, not refused");
    let (row_version, row_digest) = row_after_ensure.expect("ensure_schema_contract must leave a stamp behind");
    assert_eq!(row_version, contract.version().get(), "the stamp must advance to the binary's version");
    assert_eq!(row_digest, contract.digest().to_string(), "the stamp must advance to the binary's digest");

    pool.close().await;
    environment.finish().await.expect("releasing exclusive ownership failed");
}

/// Verification matrix row "Schema contract drift": the production entry
/// point refuses a tenant migrated along a different path rather than
/// restamping over it.
#[tokio::test]
async fn ensure_schema_contract_refuses_a_tenant_stamped_with_another_migration_set_and_leaves_the_drift_untouched() {
    let configuration = TestRunConfiguration::from_environment().expect("integration test configuration");
    let environment = SerializedEnvironment::acquire(&configuration).await.expect("exclusive ownership of the stamp");
    let pool = connect_test_database().await.expect("test database connection");
    let contract = embedded_contract();

    // A digest of the right shape that no migration set produces: the column's
    // check constraint accepts it, so the refusal comes from the contract check
    // rather than from the database.
    let (refusal, row_after_ensure) = under_drift_through_ensure(&pool, "UPDATE schema_metadata SET migrations_sha256 = repeat('a', 64) WHERE singleton").await;

    match refusal {
        Err(SchemaContractError::MigrationsDigestMismatch { expected, found }) => {
            assert_eq!(expected, contract.digest(), "the refusal must name the digest the binary requires");
            assert_eq!(found.to_string(), "a".repeat(64), "the refusal must name the digest the tenant carries");
        }
        Err(other) => unreachable!("a digest mismatch must be reported as such: {other}"),
        Ok(()) => unreachable!("a tenant migrated along another path must be refused, not silently restamped"),
    }
    let (row_version, row_digest) = row_after_ensure.expect("a stamp present before the call must still be present after a refusal");
    assert_eq!(
        row_version,
        contract.version().get(),
        "the version column was never drifted and must stay untouched"
    );
    assert_eq!(
        row_digest,
        "a".repeat(64),
        "a refused ensure_schema_contract must not overwrite the drifted digest"
    );

    pool.close().await;
    environment.finish().await.expect("releasing exclusive ownership failed");
}

/// Verification matrix row "Schema contract drift": the production entry
/// point stamps a tenant that has genuinely never carried one, which is the
/// only state `ensure_schema_contract` is allowed to write.
#[tokio::test]
async fn ensure_schema_contract_stamps_a_tenant_that_has_never_carried_a_stamp() {
    let configuration = TestRunConfiguration::from_environment().expect("integration test configuration");
    let environment = SerializedEnvironment::acquire(&configuration).await.expect("exclusive ownership of the stamp");
    let pool = connect_test_database().await.expect("test database connection");
    let contract = embedded_contract();

    let (outcome, row_after_ensure) = under_drift_through_ensure(&pool, "DELETE FROM schema_metadata").await;

    outcome.expect("a tenant that was never stamped must be stamped and accepted, not refused");
    let (row_version, row_digest) = row_after_ensure.expect("ensure_schema_contract must have written a stamp");
    assert_eq!(row_version, contract.version().get(), "the fresh stamp must carry the binary's version");
    assert_eq!(row_digest, contract.digest().to_string(), "the fresh stamp must carry the binary's digest");

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
