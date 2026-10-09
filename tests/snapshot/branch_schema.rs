//! Snapshot branch schemas: built by the branch migrations, upgraded by them,
//! and quarantined when an upgrade fails.
//!
//! A Snapshot's branch schema is created by the same branch migration files
//! that create the live branch in `public`, so its shape is the live shape
//! name for name, and a branch migration a later release appends reaches every
//! existing Snapshot through the upgrade path. A Snapshot whose upgrade fails
//! is refused for scoping as an internal error naming it while the tenant and
//! every other Snapshot keep serving, and a re-run after the cause is removed
//! lifts the quarantine.

use crate::support::{
    assert_problem_details,
    broker_url,
    clean_database,
    client,
    create_entity,
    create_snapshot_raw,
    delete_snapshot_raw,
    published_branch_schema,
    query_snapshot_entities,
    wait_for_snapshot_terminal,
};
use andromeda_postgresql_context_store::{
    migration_set::branch::{BRANCH_LEDGER, BranchMigrationSet, branch_migrations, current_branch_version},
    snapshot::branch_upgrade::{BranchUpgradeReport, SnapshotBranchState, snapshot_branch_state, upgrade_snapshot_branches},
};
use regex::Regex;
use reqwest::StatusCode;
use serde_json::{Value, json};
use serial_test::serial;
use sqlx::{
    PgPool,
    migrate::{Migration, MigrationType},
};
use std::{borrow::Cow, env, time::Duration};
use tokio::time::timeout;

/// The foreign keys from live branch relations into the tenant-global
/// dictionaries, which the live-only tenant migration declares and no
/// Snapshot branch carries.
const LIVE_ONLY_DICTIONARY_REFERENCES: [&str; 3] = [
    "fk_attribute_history_instance_claims_attribute_name",
    "fk_entity_type_members_entity_type_name",
    "fk_temporal_attribute_streams_attribute_name",
];

fn minimal(id: &str) -> Value {
    json!({"id": id, "type": "Snapshot", "snapshotQueries": []})
}

async fn snapshot_database() -> PgPool {
    PgPool::connect(&env::var("ANDROMEDA_TEST_DATABASE_URL").expect("Snapshot database URL"))
        .await
        .expect("Snapshot database connection")
}

/// Create a prepared Snapshot and return the name of its published branch schema.
async fn prepared_snapshot(pool: &PgPool, id: &str) -> String {
    pretty_assertions::assert_eq!(create_snapshot_raw(&client(), &minimal(id)).await.0, StatusCode::CREATED);
    wait_for_snapshot_terminal(&client(), id, Duration::from_secs(5)).await;
    published_branch_schema(pool, id).await
}

/// One relation's shape in `schema`, with every mention of the schema itself
/// removed so two schemas holding the same relation compare equal. The live
/// dictionary references are left out: they are the one intended difference.
async fn relation_shape(pool: &PgPool, schema: &str, table: &str) -> Vec<String> {
    let columns: Vec<(String, String, String, Option<String>, String, String)> = sqlx::query_as(
        "SELECT column_name::text, udt_name::text, is_nullable::text, column_default::text, is_identity::text, is_generated::text \
         FROM information_schema.columns WHERE table_schema = $1 AND table_name = $2 ORDER BY ordinal_position",
    )
    .bind(schema)
    .bind(table)
    .fetch_all(pool)
    .await
    .expect("column catalogue");
    let indexes: Vec<(String, String)> =
        sqlx::query_as("SELECT indexname::text, indexdef FROM pg_indexes WHERE schemaname = $1 AND tablename = $2 ORDER BY indexname")
            .bind(schema)
            .bind(table)
            .fetch_all(pool)
            .await
            .expect("index catalogue");
    let constraints: Vec<(String, String)> = sqlx::query_as(
        "SELECT c.conname::text, pg_get_constraintdef(c.oid) FROM pg_constraint AS c \
         JOIN pg_class AS r ON r.oid = c.conrelid JOIN pg_namespace AS n ON n.oid = r.relnamespace \
         WHERE n.nspname = $1 AND r.relname = $2 AND c.conname <> ALL($3) ORDER BY c.conname",
    )
    .bind(schema)
    .bind(table)
    .bind(LIVE_ONLY_DICTIONARY_REFERENCES)
    .fetch_all(pool)
    .await
    .expect("constraint catalogue");
    let unqualified = |text: String| text.replace(&format!("{schema}."), "").replace("public.", "");
    columns
        .into_iter()
        .map(|(name, udt, nullable, default, identity, generated)| format!("column {name} {udt} {nullable} {default:?} {identity} {generated}"))
        .chain(indexes.into_iter().map(|(name, definition)| format!("index {name} {definition}")))
        .chain(constraints.into_iter().map(|(name, definition)| format!("constraint {name} {definition}")))
        .map(unqualified)
        .collect()
}

#[tokio::test]
#[serial(snapshot)]
async fn snapshot_branch_schema_matches_the_live_branch_name_for_name() {
    let _serialized_environment = clean_database().await;
    let pool = snapshot_database().await;
    let schema = prepared_snapshot(&pool, "urn:ngsi-ld:Snapshot:branch-shape").await;

    let tables: Vec<String> = sqlx::query_scalar("SELECT tablename::text FROM pg_tables WHERE schemaname = $1 AND tablename <> $2 ORDER BY tablename")
        .bind(&schema)
        .bind(BRANCH_LEDGER)
        .fetch_all(&pool)
        .await
        .expect("branch relations");
    assert!(!tables.is_empty(), "the branch schema {schema} holds the branch relations");
    // Only the live branch defers work: a Snapshot branch runs its side effects
    // inline and records its history in the write's own transaction, so it has
    // neither queue.
    for live_only in ["pending_side_effects", "temporal_append_intents"] {
        assert!(!tables.iter().any(|table| table == live_only), "{schema} holds no {live_only}");
    }
    for table in &tables {
        pretty_assertions::assert_eq!(
            relation_shape(&pool, &schema, table).await,
            relation_shape(&pool, "public", table).await,
            "{schema}.{table} must have exactly the live shape"
        );
    }

    let recorded: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(format!("SELECT COUNT(*) FROM {schema}.{BRANCH_LEDGER}")))
        .fetch_one(&pool)
        .await
        .expect("branch ledger");
    pretty_assertions::assert_eq!(recorded, i64::try_from(branch_migrations().len()).expect("count fits"));
    pretty_assertions::assert_eq!(
        delete_snapshot_raw(&client(), "urn:ngsi-ld:Snapshot:branch-shape").await.0,
        StatusCode::NO_CONTENT
    );
}

/// The `relation.constraint -> referenced relation` of every foreign key a
/// relation in `schema` declares, the referenced relation schema-qualified.
async fn foreign_keys(pool: &PgPool, schema: &str) -> Vec<String> {
    sqlx::query_scalar(
        "SELECT r.relname::text || '.' || c.conname::text || ' -> ' || c.confrelid::regclass::text FROM pg_constraint AS c \
         JOIN pg_class AS r ON r.oid = c.conrelid JOIN pg_namespace AS n ON n.oid = r.relnamespace \
         WHERE n.nspname = $1 AND c.contype = 'f' ORDER BY 1",
    )
    .bind(schema)
    .fetch_all(pool)
    .await
    .expect("foreign key catalogue")
}

/// Clause 5.16.1.4 prepares a Snapshot in the background of the live context,
/// and clause 5.6.1.4 leaves a create no failure an unrelated Snapshot could
/// cause, so a Snapshot branch's DDL must never lock a tenant-global relation
/// against live writes. A foreign key locks the relation it references each
/// time its table is created or dropped; the branch therefore references only
/// its own relations, and the dictionary references live on the live branch
/// alone.
#[tokio::test]
#[serial(snapshot)]
async fn snapshot_branch_schema_references_only_its_own_relations() {
    let _serialized_environment = clean_database().await;
    let pool = snapshot_database().await;
    let schema = prepared_snapshot(&pool, "urn:ngsi-ld:Snapshot:branch-references").await;

    let branch_keys = foreign_keys(&pool, &schema).await;
    assert!(!branch_keys.is_empty(), "the branch schema {schema} declares its own foreign keys");
    let escaping: Vec<&String> = branch_keys.iter().filter(|key| !key.contains(&format!(" -> {schema}."))).collect();
    assert!(escaping.is_empty(), "{schema} references relations outside itself: {escaping:#?}");

    let live_keys = foreign_keys(&pool, "public").await;
    for (constraint, referenced) in LIVE_ONLY_DICTIONARY_REFERENCES
        .iter()
        .zip(["attribute_names", "entity_type_names", "attribute_names"])
    {
        assert!(
            live_keys.iter().any(|key| key.contains(&format!(".{constraint} -> {referenced}"))),
            "the live branch declares {constraint} onto {referenced}: {live_keys:#?}"
        );
    }
    pretty_assertions::assert_eq!(
        delete_snapshot_raw(&client(), "urn:ngsi-ld:Snapshot:branch-references").await.0,
        StatusCode::NO_CONTENT
    );
}

/// A session holding `ROW EXCLUSIVE` on both dictionaries -- the lock every
/// live write that mints a name or reads a unit code is compatible with --
/// conflicts with every lock a foreign key's creation or removal takes on the
/// relation it references. Creating, preparing and deleting a Snapshot while
/// it is held therefore proves that no step of a Snapshot's lifetime locks the
/// dictionaries: with a branch key into either one, preparation stalls until
/// the session ends and the deletion never returns.
#[tokio::test]
#[serial(snapshot)]
async fn snapshot_lifecycle_completes_while_the_dictionaries_are_row_exclusive_locked() {
    let _serialized_environment = clean_database().await;
    let pool = snapshot_database().await;
    let id = "urn:ngsi-ld:Snapshot:dictionaries-held";
    let mut holder = pool.begin().await.expect("lock holder transaction");
    sqlx::query("LOCK TABLE attribute_names, unit_codes IN ROW EXCLUSIVE MODE")
        .execute(&mut *holder)
        .await
        .expect("hold the dictionaries");

    // A live write interning a fresh type, a fresh attribute and a unit code
    // is compatible with the held lock; the Snapshot then captures it.
    let entity_type = "https://example.org/DictionaryHeldVehicle";
    let entity = json!({
        "id": "urn:ngsi-ld:DictionaryHeldVehicle:1",
        "type": entity_type,
        "https://example.org/dictionaryHeldSpeed": {"type": "Property", "value": 42, "unitCode": "KMH"},
        "@context": "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld"
    });
    let live_write = timeout(Duration::from_secs(5), create_entity(&client(), &entity))
        .await
        .expect("a live write never waits on the held lock");
    pretty_assertions::assert_eq!(live_write, StatusCode::CREATED);

    let body = json!({"id": id, "type": "Snapshot", "snapshotQueries": [{"type": "Query", "entities": [{"type": entity_type}]}]});
    let created = timeout(Duration::from_secs(5), create_snapshot_raw(&client(), &body))
        .await
        .expect("the create never waits on the dictionaries");
    pretty_assertions::assert_eq!(created.0, StatusCode::CREATED);
    let prepared = wait_for_snapshot_terminal(&client(), id, Duration::from_secs(5)).await;
    pretty_assertions::assert_eq!(prepared["snapshotStatus"], "success", "{prepared}");
    let deleted = timeout(Duration::from_secs(5), delete_snapshot_raw(&client(), id))
        .await
        .expect("the deletion never waits on the dictionaries");
    pretty_assertions::assert_eq!(deleted.0, StatusCode::NO_CONTENT);

    holder.rollback().await.expect("release the dictionaries");
}

/// The relations `migration` creates, in the order it creates them.
fn created_relations(migration: &Migration) -> Vec<String> {
    Regex::new(r"(?i)\bCREATE\s+(?:UNLOGGED\s+)?TABLE\s+(?:IF\s+NOT\s+EXISTS\s+)?([A-Za-z_][A-Za-z0-9_]*)")
        .expect("the relation pattern is static and valid")
        .captures_iter(migration.sql.as_str())
        .filter_map(|created| created.get(1))
        .map(|name| name.as_str().to_owned())
        .collect()
}

/// A branch migration a later release appends: one new column on a branch relation.
fn appended_migration(version: i64) -> Migration {
    Migration::new(
        version,
        Cow::Borrowed("appended branch column"),
        MigrationType::Simple,
        sqlx::SqlStr::from_static("ALTER TABLE entities ADD COLUMN appended_branch_probe INTEGER;"),
        false,
    )
}

#[tokio::test]
#[serial(snapshot)]
async fn snapshot_branch_upgrade_applies_an_appended_branch_migration_to_every_snapshot() {
    let _serialized_environment = clean_database().await;
    let pool = snapshot_database().await;
    let first = prepared_snapshot(&pool, "urn:ngsi-ld:Snapshot:upgrade-first").await;
    let second = prepared_snapshot(&pool, "urn:ngsi-ld:Snapshot:upgrade-second").await;

    let appended = i64::from(current_branch_version().expect("branch version").get()) + 1;
    let mut migrations = branch_migrations().to_vec();
    migrations.push(appended_migration(appended));
    let report = upgrade_snapshot_branches(&pool, &BranchMigrationSet::new(migrations))
        .await
        .expect("upgrade run");
    pretty_assertions::assert_eq!(report, BranchUpgradeReport { upgraded: 2, quarantined: 0 });

    for schema in [&first, &second] {
        let column: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM information_schema.columns WHERE table_schema = $1 AND table_name = 'entities' AND column_name = 'appended_branch_probe')",
        )
        .bind(schema)
        .fetch_one(&pool)
        .await
        .expect("column catalogue");
        assert!(column, "{schema} carries the appended column");
        let ledgered: bool = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "SELECT EXISTS (SELECT 1 FROM {schema}.{BRANCH_LEDGER} WHERE version = $1)"
        )))
        .bind(appended)
        .fetch_one(&pool)
        .await
        .expect("branch ledger");
        assert!(ledgered, "{schema} records the appended migration in its own ledger");
    }
    let versions: Vec<i32> = sqlx::query_scalar("SELECT branch_schema_version FROM snapshot_preparation ORDER BY snapshot_key")
        .fetch_all(&pool)
        .await
        .expect("branch versions");
    pretty_assertions::assert_eq!(versions, vec![i32::try_from(appended).expect("version fits"); 2]);

    for id in ["urn:ngsi-ld:Snapshot:upgrade-first", "urn:ngsi-ld:Snapshot:upgrade-second"] {
        pretty_assertions::assert_eq!(delete_snapshot_raw(&client(), id).await.0, StatusCode::NO_CONTENT);
    }
}

#[tokio::test]
#[serial(snapshot)]
async fn snapshot_branch_upgrade_failure_quarantines_only_that_snapshot_until_a_rerun_succeeds() {
    let _serialized_environment = clean_database().await;
    let pool = snapshot_database().await;
    let broken_id = "urn:ngsi-ld:Snapshot:quarantine-broken";
    let healthy_id = "urn:ngsi-ld:Snapshot:quarantine-healthy";
    let broken = prepared_snapshot(&pool, broken_id).await;
    prepared_snapshot(&pool, healthy_id).await;

    // A Snapshot made by a release one branch migration behind, whose schema
    // nevertheless already holds that migration's relation: re-applying the
    // migration fails, which is a failed upgrade.
    let last = branch_migrations().last().expect("the branch set is not empty");
    assert!(
        !created_relations(last).is_empty(),
        "the quarantine is provoked by re-applying a migration whose relations already exist, so the last branch migration ({}) must create one",
        last.version
    );
    let current = current_branch_version().expect("branch version").get();
    sqlx::query(sqlx::AssertSqlSafe(format!("DELETE FROM {broken}.{BRANCH_LEDGER} WHERE version = $1")))
        .bind(last.version)
        .execute(&pool)
        .await
        .expect("forget the last branch migration");
    sqlx::query("UPDATE snapshot_preparation SET branch_schema_version = $2 WHERE snapshot_key = (SELECT snapshot_key FROM snapshots WHERE id = $1)")
        .bind(broken_id)
        .bind(current - 1)
        .execute(&pool)
        .await
        .expect("record the older branch version");
    pretty_assertions::assert_eq!(snapshot_branch_state(&pool).await.expect("probe"), SnapshotBranchState::Behind);

    let report = upgrade_snapshot_branches(&pool, &BranchMigrationSet::embedded()).await.expect("upgrade run");
    pretty_assertions::assert_eq!(report, BranchUpgradeReport { upgraded: 0, quarantined: 1 });
    let recorded: Option<String> = sqlx::query_scalar(
        "SELECT p.branch_upgrade_error FROM snapshot_preparation AS p JOIN snapshots AS s ON s.snapshot_key = p.snapshot_key WHERE s.id = $1",
    )
    .bind(broken_id)
    .fetch_one(&pool)
    .await
    .expect("upgrade error");
    assert!(recorded.is_some_and(|error| !error.is_empty()), "the failed upgrade records why");

    let (status, _, problem) = query_snapshot_entities(&client(), broken_id, "type=Vehicle").await;
    pretty_assertions::assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_problem_details(&problem, "https://uri.etsi.org/ngsi-ld/errors/InternalError", StatusCode::INTERNAL_SERVER_ERROR);
    assert!(problem["detail"].as_str().is_some_and(|detail| detail.contains(broken_id)), "{problem}");
    pretty_assertions::assert_eq!(query_snapshot_entities(&client(), healthy_id, "type=Vehicle").await.0, StatusCode::OK);
    let live = client().get(broker_url("/entities?type=Vehicle")).send().await.expect("live query");
    pretty_assertions::assert_eq!(live.status(), StatusCode::OK);

    // Once the cause is removed, a re-run upgrades the Snapshot and lifts the
    // quarantine: the conflicting relations are the ones the last branch
    // migration creates.
    let conflicting = created_relations(last)
        .iter()
        .map(|relation| format!("{broken}.{relation}"))
        .collect::<Vec<_>>()
        .join(", ");
    sqlx::query(sqlx::AssertSqlSafe(format!("DROP TABLE {conflicting} CASCADE")))
        .execute(&pool)
        .await
        .expect("remove the conflicting relations");
    let report = upgrade_snapshot_branches(&pool, &BranchMigrationSet::embedded()).await.expect("upgrade rerun");
    pretty_assertions::assert_eq!(report, BranchUpgradeReport { upgraded: 1, quarantined: 0 });
    pretty_assertions::assert_eq!(snapshot_branch_state(&pool).await.expect("probe"), SnapshotBranchState::Current);
    let (version, error): (i32, Option<String>) = sqlx::query_as(
        "SELECT p.branch_schema_version, p.branch_upgrade_error FROM snapshot_preparation AS p JOIN snapshots AS s ON s.snapshot_key = p.snapshot_key WHERE s.id = $1",
    )
    .bind(broken_id)
    .fetch_one(&pool)
    .await
    .expect("upgrade outcome");
    pretty_assertions::assert_eq!((version, error), (current, None));
    pretty_assertions::assert_eq!(query_snapshot_entities(&client(), broken_id, "type=Vehicle").await.0, StatusCode::OK);

    for id in [broken_id, healthy_id] {
        pretty_assertions::assert_eq!(delete_snapshot_raw(&client(), id).await.0, StatusCode::NO_CONTENT);
    }
}
