//! Verification-matrix row "Object GIN operator class".
//!
//! `jsonb_path_ops` indexes containment (`@>`) and nothing else; the default
//! `jsonb_ops` class also serves key existence (`?`, `?|`, `?&`). The schema
//! deliberately carries no GIN index on `current_attribute_values.value_object`
//! because no predicate is compiled against it, so the test states both halves:
//! the absent index has no silent consumer, and the two operator classes behave
//! as declared when they are present.

use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use sqlx::{Executor, PgConnection};

/// Statements building a two-index fixture whose plans distinguish the operator
/// classes. It lives in a temporary relation so the production schema keeps the
/// index set its consumers justify.
const FIXTURE: [&str; 5] = [
    "CREATE TEMPORARY TABLE object_predicate_fixture (id BIGINT PRIMARY KEY, containment JSONB NOT NULL, existence JSONB NOT NULL) ON COMMIT DROP",
    "INSERT INTO object_predicate_fixture (id, containment, existence) \
     SELECT g, jsonb_build_object('city', 'city-' || (g % 5000)::text), jsonb_build_object('city', 'city-' || (g % 5000)::text) \
     FROM generate_series(1, 20000) AS g",
    "CREATE INDEX object_predicate_path_ops ON object_predicate_fixture USING GIN (containment jsonb_path_ops)",
    "CREATE INDEX object_predicate_default_ops ON object_predicate_fixture USING GIN (existence)",
    "ANALYZE object_predicate_fixture",
];

/// Run `EXPLAIN` on one connection so a temporary relation stays visible.
async fn explain_on(connection: &mut PgConnection, statement: &str) -> String {
    let plans: Vec<String> = sqlx::query_scalar(&format!("EXPLAIN (ANALYZE, BUFFERS) {statement}"))
        .fetch_all(&mut *connection)
        .await
        .expect("the plan must be readable");
    plans.join("\n")
}

#[tokio::test]
async fn object_containment_uses_path_ops_and_key_existence_needs_the_default_class() {
    let scope = TestEnvironment::from_suite("query-access-paths").await.expect("isolated test environment");
    // One transaction, so the temporary relation and its indexes live on a
    // single connection and disappear at rollback.
    let mut transaction = scope.database_pool().begin().await.expect("transaction must start");
    for statement in FIXTURE {
        transaction.execute(statement).await.expect("fixture statement must succeed");
    }

    let containment = explain_on(
        &mut transaction,
        "SELECT id FROM object_predicate_fixture WHERE containment @> '{\"city\": \"city-42\"}'::jsonb",
    )
    .await;
    assert!(
        containment.contains("object_predicate_path_ops"),
        "a containment predicate must use the jsonb_path_ops index: {containment}"
    );

    let existence = explain_on(&mut transaction, "SELECT id FROM object_predicate_fixture WHERE existence ? 'city'").await;
    assert!(
        !existence.contains("object_predicate_path_ops"),
        "key existence must never be answered by an index class that cannot serve it: {existence}"
    );

    // The residual oracle: both predicates select the same rows the planner
    // claims, whichever access path it chooses.
    let contained: i64 = sqlx::query_scalar("SELECT count(*) FROM object_predicate_fixture WHERE containment @> '{\"city\": \"city-42\"}'::jsonb")
        .fetch_one(&mut *transaction)
        .await
        .expect("containment count must be readable");
    assert_eq!(contained, 4, "20000 rows over 5000 distinct cities give four matches");

    transaction.rollback().await.expect("rollback must succeed");

    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn the_projection_carries_no_unconsumed_object_index() {
    let scope = TestEnvironment::from_suite("query-access-paths").await.expect("isolated test environment");

    let object_indexes: Vec<String> = sqlx::query_scalar(
        "SELECT c.relname FROM pg_index AS i \
         JOIN pg_class AS c ON c.oid = i.indexrelid \
         JOIN pg_class AS t ON t.oid = i.indrelid \
         WHERE t.relname = 'current_attribute_values' \
           AND pg_get_indexdef(i.indexrelid) LIKE '%value_object%'",
    )
    .fetch_all(scope.database_pool())
    .await
    .expect("index list must be readable");
    assert_eq!(
        object_indexes,
        Vec::<String>::new(),
        "no predicate compiles against value_object, so it must carry no index to miss"
    );

    scope.finish().await.expect("cleanup failed");
}
