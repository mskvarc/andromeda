//! Identifier collation: every identifier column compares byte-wise.
//!
//! Every identifier column is `COLLATE "C"`, so comparison is byte-wise: an
//! anchored `idPattern` (clause 4.9) becomes a plain B-tree range scan with no
//! pattern-operator index, an unanchored one falls to the trigram index, and
//! ordering is the byte order a test can reproduce in Rust.

use crate::seeded_corpus::{SEED_PREFIX, explain, seed};
use andromeda_test_harness::serialized_environment::SerializedEnvironment;
use pretty_assertions::assert_eq;
use sqlx::PgPool;

/// Read the collation name of one column, absent when the column is not
/// collatable.
async fn column_collation(pool: &PgPool, relation: &str, column: &str) -> Option<String> {
    sqlx::query_scalar(
        "SELECT co.collname FROM pg_attribute AS a \
         JOIN pg_class AS c ON c.oid = a.attrelid \
         JOIN pg_collation AS co ON co.oid = a.attcollation \
         WHERE c.relname = $1 AND a.attname = $2 AND a.attnum > 0 AND NOT a.attisdropped",
    )
    .bind(relation)
    .bind(column)
    .fetch_optional(pool)
    .await
    .expect("the column must exist")
    .flatten()
}

#[tokio::test]
async fn identifier_columns_declare_byte_wise_collation() {
    let scope = SerializedEnvironment::from_environment_reset().await.expect("serialized test environment");
    let pool = scope.database_pool();

    for (relation, column) in [
        ("entities", "id"),
        ("temporal_entities", "id"),
        ("subscriptions", "id"),
        ("attribute_names", "name"),
        ("current_attributes", "dataset_id"),
        ("current_attributes", "value_string"),
        ("current_attributes", "value_uri"),
        ("current_attributes", "value_long_text"),
        ("current_attributes", "value_long_uri"),
        ("attribute_history", "value_string"),
        ("attribute_history", "value_uri"),
        ("attribute_history", "verbatim_instance_id"),
        ("attribute_history_instance_claims", "verbatim_instance_id"),
        ("scope_history", "verbatim_instance_id"),
        ("temporal_attribute_streams", "dataset_id"),
        ("registration_information", "entity_types"),
        ("registration_information", "entity_id"),
        ("registration_information", "registration_id"),
        ("context_source_registrations", "id"),
        ("snapshots", "id"),
        ("jsonld_contexts", "id"),
        ("jsonld_contexts", "url"),
        ("notification_events", "entity_id"),
        ("temporal_append_intents", "entity_id"),
        ("schema_metadata", "migrations_sha256"),
    ] {
        assert_eq!(
            column_collation(pool, relation, column).await.as_deref(),
            Some("C"),
            "{relation}.{column} must compare byte-wise, not by locale"
        );
    }

    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn identifier_patterns_plan_as_a_range_scan_when_anchored_and_a_trigram_scan_otherwise() {
    let scope = SerializedEnvironment::from_environment_reset().await.expect("serialized test environment");
    let pool = scope.database_pool();
    let corpus = seed(pool, SEED_PREFIX).await;
    let prefix = corpus.prefix();

    let anchored = explain(pool, &format!("SELECT e.id FROM entities AS e WHERE e.id LIKE '{prefix}:seed:0001%'")).await;
    assert!(
        anchored.contains("uq_entities_id"),
        "an anchored pattern must range-scan the identifier B-tree: {anchored}"
    );
    assert!(
        anchored.contains("Index Cond") && anchored.contains(">=") && anchored.contains('<'),
        "the anchored pattern must become range bounds, not a filter: {anchored}"
    );
    assert!(
        !anchored.contains("text_pattern_ops"),
        "byte-wise collation makes a pattern-operator index unnecessary: {anchored}"
    );

    let unanchored = explain(pool, "SELECT e.id FROM entities AS e WHERE e.id ~ 'seed:01234'").await;
    assert!(
        unanchored.contains("idx_entities_id_trgm"),
        "an unanchored pattern has no usable anchor and must use the trigram index: {unanchored}"
    );

    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn identifier_equality_and_ordering_match_the_byte_order_oracle() {
    let scope = SerializedEnvironment::from_environment_reset().await.expect("serialized test environment");
    let pool = scope.database_pool();
    let corpus = seed(pool, SEED_PREFIX).await;

    let ordered: Vec<String> = sqlx::query_scalar("SELECT id FROM entities WHERE id LIKE $1::text || ':seed:%' ORDER BY id LIMIT 500")
        .bind(corpus.prefix())
        .fetch_all(pool)
        .await
        .expect("ordered identifiers must be readable");

    let mut oracle = ordered.clone(); // clone: the oracle sorts an owned copy so the database order stays comparable.
    oracle.sort_by(|left, right| left.as_bytes().cmp(right.as_bytes()));
    assert_eq!(ordered, oracle, "ordering must be byte order, not a locale order");

    let exact = corpus.entity_id(7);
    let found: Option<String> = sqlx::query_scalar("SELECT id FROM entities WHERE id = $1")
        .bind(&exact)
        .fetch_optional(pool)
        .await
        .expect("equality lookup must succeed");
    assert_eq!(found.as_deref(), Some(exact.as_str()), "equality must return the byte-identical identifier");

    scope.finish().await.expect("cleanup failed");
}
