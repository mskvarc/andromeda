//! Migration failure: a tenant left half-switched by a failed migration is
//! never served.
//!
//! The rebuild is a maintenance-window cutover, so the failure mode that
//! matters is a tenant left half-switched: some phases applied, some not. Three
//! properties keep that state from being served.
//!
//! First, a phase that fails leaves nothing behind. Every migration is sent as
//! one multi-statement simple query, which PostgreSQL wraps in an implicit
//! transaction, so a statement that raises rolls the whole file back. The
//! exceptions are the files that declare `-- no-transaction` because
//! `CREATE INDEX CONCURRENTLY` cannot run inside a block, and each of those
//! holds exactly one statement precisely so a failure has nothing partial to
//! leave.
//!
//! Second, a tenant whose migration set stopped part-way is stamped with a
//! version this binary does not require, and the contract check refuses it
//! before the pool is published: the broker does not start against a partially
//! switched schema.
//!
//! Third, the cutover leaves no second authority to fall back on — no view
//! carrying an old relation's name over a new one — so an operator's only
//! recovery is the runbook's verified backup.

use crate::{
    SUITE,
    schema_stamp::{read_stamp_row, restamp, restamp_version},
};
use andromeda_postgresql_context_store::{
    migration_set::tenant::tenant_migrator,
    schema_contract::{contract::SchemaContract, error::SchemaContractError, schema_metadata::verify_schema_contract},
};
use andromeda_postgresql_seaquery::{
    aggregate::count_all,
    statement::Statement,
    system_catalog::{InformationSchema, InformationSchemaViews},
};
use andromeda_test_harness::{
    configuration::TestRunConfiguration,
    database_access::connect_test_database,
    environment::TestEnvironment,
    serialized_environment::SerializedEnvironment,
};
use pretty_assertions::assert_eq;
use sea_query::{Expr, ExprTrait, Query};
use sqlx::PgPool;
use std::{fs, path::Path};

/// The directories the tenant migrator runs, as one version space.
const TENANT_MIGRATION_DIRECTORIES: [&str; 2] = ["migrations/tenant", "migrations/branch"];

/// The marker a migration declares when it cannot run inside a transaction.
const NO_TRANSACTION: &str = "-- no-transaction";

/// The relations the rebuild replaced, which must not survive as views.
const REPLACED_RELATIONS: &[&str] = &["attribute_current", "temporal_entity_attributes", "notification_outbox"];

/// The contract the embedded migration set expresses.
fn embedded_contract() -> SchemaContract {
    SchemaContract::of(tenant_migrator()).expect("the embedded tenant migration set expresses a contract")
}

/// One migration body with its line comments removed.
///
/// Every migration in this tree opens with a comment block that names the
/// statements it does and does not carry, so a check that reads the raw text
/// would be answering about the prose rather than about the SQL.
fn sql_only(body: &str) -> String {
    body.lines().map(|line| line.split("--").next().unwrap_or("")).collect::<Vec<&str>>().join("\n")
}

/// The number of statements one migration body holds, counting a trailing
/// semicolon as a terminator rather than as a separator.
fn statement_count(body: &str) -> usize {
    sql_only(body).split(';').filter(|statement| !statement.trim().is_empty()).count()
}

/// Stamp a partially applied version, verify, and put the original stamp back.
///
/// The result is returned rather than asserted on inside, so the restore
/// happens whatever the verification said: a test that failed while the tenant
/// was left drifted would refuse the broker its next start.
async fn under_partial_cutover(pool: &PgPool, applied_version: i32) -> Result<(), SchemaContractError> {
    let stamped = read_stamp_row(pool).await;

    restamp_version(applied_version)
        .to_query()
        .execute(pool)
        .await
        .expect("the partial-cutover stamp must apply");
    let mut connection = pool.acquire().await.expect("a connection to verify the contract over");
    let verified = verify_schema_contract(&mut connection, &embedded_contract()).await;
    drop(connection);

    if let Some((version, digest)) = stamped {
        restamp(version, digest)
            .to_query()
            .execute(pool)
            .await
            .expect("the original stamp must be restorable");
    }
    verified
}

/// A phase that fails must leave nothing behind. Every migration either runs
/// inside the implicit transaction the runner's simple query creates, or
/// declares that it cannot and holds exactly one statement.
#[tokio::test]
async fn every_migration_is_all_or_nothing_so_a_failed_phase_leaves_no_partial_schema() {
    let mut inspected = 0usize;

    let paths = TENANT_MIGRATION_DIRECTORIES.iter().flat_map(|directory| {
        fs::read_dir(Path::new(env!("CARGO_MANIFEST_DIR")).join(directory))
            .expect("the tenant and branch migration directories must be readable")
            .map(|entry| entry.expect("each migration entry must be readable").path())
    });
    for path in paths {
        if path.extension().and_then(|extension| extension.to_str()) != Some("sql") {
            continue;
        }
        let body = fs::read_to_string(&path).expect("each migration must be readable");
        let name = path.file_name().and_then(|name| name.to_str()).unwrap_or_default().to_owned();
        inspected += 1;

        if body.trim_start().starts_with(NO_TRANSACTION) {
            assert_eq!(
                statement_count(&body),
                1,
                "{name} runs outside a transaction, so it must hold exactly one statement to be all-or-nothing"
            );
        } else {
            assert!(
                !sql_only(&body).contains("CONCURRENTLY"),
                "{name} runs inside a transaction, where no CONCURRENTLY statement is allowed"
            );
        }
    }

    assert!(inspected > 0, "the tenant and branch migration directories must hold migrations");
}

/// A tenant whose cutover stopped between phases is stamped with a version
/// this binary does not require, and the contract check names it rather than
/// letting the broker serve a half-switched schema.
#[tokio::test]
async fn a_tenant_left_between_cutover_phases_is_refused_at_startup() {
    let configuration = TestRunConfiguration::from_environment().expect("integration test configuration");
    let environment = SerializedEnvironment::acquire(&configuration).await.expect("exclusive ownership of the stamp");
    let pool = connect_test_database().await.expect("test database connection");
    let contract = embedded_contract();

    // One version short of the set this binary embeds: every phase up to the
    // last one applied, which is exactly the state an interrupted cutover
    // leaves.
    let partial = contract.version().get() - 1;
    let refusal = under_partial_cutover(&pool, partial).await;

    match refusal {
        Err(SchemaContractError::SchemaVersionMismatch { expected, found }) => {
            assert_eq!(expected, contract.version(), "the refusal must name the version the binary requires");
            assert_eq!(found.get(), partial, "the refusal must name the version the tenant stopped at");
        }
        Err(other) => panic!("a partially applied migration set must be reported as a version mismatch: {other}"),
        Ok(()) => panic!("a tenant left between cutover phases must be refused"),
    }

    pool.close().await;
    environment.finish().await.expect("releasing exclusive ownership failed");
}

/// The cutover removes what it replaced instead of leaving a view behind. A
/// compatibility view would give the tenant two authorities for one fact and
/// would let a half-restored database look healthy.
#[tokio::test]
async fn the_cutover_leaves_no_compatibility_view_over_a_replaced_relation() {
    let scope = TestEnvironment::from_suite(SUITE).await.expect("isolated test environment");

    for relation in REPLACED_RELATIONS {
        let surviving: i64 = Query::select()
            .expr(count_all())
            .from((InformationSchema, InformationSchemaViews::Table))
            .and_where(Expr::col(InformationSchemaViews::TableSchema).eq("public"))
            .and_where(Expr::col(InformationSchemaViews::TableName).eq(*relation))
            .to_query_scalar()
            .fetch_one(scope.database_pool())
            .await
            .expect("the view catalogue must be readable");
        assert_eq!(surviving, 0, "{relation} must not survive the cutover as a compatibility view");
    }

    scope.finish().await.expect("cleanup failed");
}
