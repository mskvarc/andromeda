//! Verification matrix row "Attribute dictionary capacity".
//!
//! The dictionary key is `INTEGER` (migration `0007_attribute_names`)
//! precisely so that a tenant modelling an open world is not stopped at the
//! 32,767 entries a `SMALLINT` key could number. The check is therefore made
//! past that boundary: 32,768 distinct valid attribute IRIs are interned, and
//! every one of them keeps a positive, distinct key that the same array-bound
//! lookup the broker issues reads back unchanged.

use crate::GENERATED_NAME_PREFIX;
use andromeda_test_harness::{configuration::TestRunConfiguration, database_access::connect_test_database, serialized_environment::SerializedEnvironment};
use pretty_assertions::assert_eq;
use sqlx::{PgPool, Row};

/// How many distinct names the capacity check interns.
///
/// One past the largest key a `SMALLINT` could hold, which is the boundary the
/// widening removed.
const GENERATED_NAMES: i32 = 32_768;

/// Names asked for in one array-bound lookup, the shape the broker's dictionary
/// cache issues when it resolves a batch of previously unseen attributes.
const LOOKUP_BATCH: i32 = 1_000;

/// Remove exactly the names this check interned.
async fn remove_generated_names(pool: &PgPool) {
    sqlx::query("DELETE FROM attribute_names WHERE name LIKE $1")
        .bind(format!("{GENERATED_NAME_PREFIX}%"))
        .execute(pool)
        .await
        .expect("the generated dictionary entries must be removable");
}

#[tokio::test]
async fn thirty_two_thousand_distinct_attribute_names_keep_positive_distinct_keys() {
    let configuration = TestRunConfiguration::from_environment().expect("integration test configuration");
    let environment = SerializedEnvironment::acquire(&configuration)
        .await
        .expect("exclusive ownership of the shared dictionary");
    let pool = connect_test_database().await.expect("test database connection");

    remove_generated_names(&pool).await;
    let interned =
        sqlx::query("INSERT INTO attribute_names (name) SELECT $1 || generated::TEXT FROM generate_series(1, $2) AS generated ON CONFLICT (name) DO NOTHING")
            .bind(GENERATED_NAME_PREFIX)
            .bind(GENERATED_NAMES)
            .execute(&pool)
            .await
            .expect("interning distinct attribute names must not fail");
    assert_eq!(
        i64::try_from(interned.rows_affected()).expect("a row count fits a signed integer"),
        i64::from(GENERATED_NAMES),
        "every generated name is distinct, so every one of them is a new dictionary entry"
    );

    let row = sqlx::query(
        "SELECT COUNT(*) AS interned, COUNT(DISTINCT id) AS distinct_keys, MIN(id) AS lowest, MAX(id) AS highest \
         FROM attribute_names WHERE name LIKE $1",
    )
    .bind(format!("{GENERATED_NAME_PREFIX}%"))
    .fetch_one(&pool)
    .await
    .expect("the dictionary must be readable");
    let interned_rows: i64 = row.get("interned");
    let distinct_keys: i64 = row.get("distinct_keys");
    let lowest: i32 = row.get("lowest");
    let highest: i32 = row.get("highest");

    assert_eq!(interned_rows, i64::from(GENERATED_NAMES), "one row per distinct name");
    assert_eq!(distinct_keys, interned_rows, "no two names may share a key");
    assert!(lowest > 0, "a dictionary key is positive, found {lowest}");
    assert!(
        highest > i32::from(i16::MAX),
        "the check has to reach past the two-byte ceiling to mean anything, highest key was {highest}"
    );

    // The lookup shape the broker's dictionary cache uses: one array bind
    // rather than a placeholder per name, which is what keeps a large batch
    // from exhausting the protocol's parameter space.
    let batch: Vec<String> = (1..=LOOKUP_BATCH).map(|position| format!("{GENERATED_NAME_PREFIX}{position}")).collect();
    let resolved = sqlx::query("SELECT id, name FROM attribute_names WHERE name = ANY($1)")
        .bind(&batch)
        .fetch_all(&pool)
        .await
        .expect("an array-bound lookup of a large batch must not fail");
    assert_eq!(resolved.len(), batch.len(), "every requested name resolves to exactly one key");
    for resolved_row in &resolved {
        let key: i32 = resolved_row.get("id");
        let name: String = resolved_row.get("name");
        assert!(key > 0, "{name} resolved to a non-positive key");
    }

    remove_generated_names(&pool).await;
    pool.close().await;
    environment.finish().await.expect("releasing exclusive ownership failed");
}

#[tokio::test]
async fn the_dictionary_key_is_declared_wide_enough_to_number_more_than_a_two_byte_key() {
    let pool = connect_test_database().await.expect("test database connection");

    let declared: String = sqlx::query_scalar(
        "SELECT data_type FROM information_schema.columns \
         WHERE table_schema = 'public' AND table_name = 'attribute_names' AND column_name = 'id'",
    )
    .fetch_one(&pool)
    .await
    .expect("the dictionary key column must be declared");
    assert_eq!(declared, "integer", "a two-byte key would cap the dictionary at 32,767 entries again");

    pool.close().await;
}
