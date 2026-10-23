//! Identifier collation: every identifier column compares byte-wise.
//!
//! Every identifier column is `COLLATE "C"`, so comparison is byte-wise: an
//! anchored `idPattern` (clause 4.9) becomes a plain B-tree range scan with no
//! pattern-operator index, an unanchored one falls to the trigram index, and
//! ordering is the byte order a test can reproduce in Rust.

use crate::seeded_corpus::{SEED_PREFIX, explain, seed};
use andromeda_postgresql_schema::current::Entities;
use andromeda_postgresql_seaquery::{operator::regex_match, statement::Statement, system_catalog::PgClass};
use andromeda_test_harness::serialized_environment::SerializedEnvironment;
use pretty_assertions::assert_eq;
use sea_query::{Alias, Expr, ExprTrait, Order, Query};
use sqlx::PgPool;

/// Read the collation name of one column, absent when the column is not
/// collatable.
async fn column_collation(pool: &PgPool, relation: &str, column: &str) -> Option<String> {
    let attribute = |name: &str| Expr::col((Alias::new("pg_attribute"), Alias::new(name)));
    Query::select()
        .column((Alias::new("pg_collation"), Alias::new("collname")))
        .from(Alias::new("pg_attribute"))
        .inner_join(
            PgClass::Table,
            Expr::col((PgClass::Table, PgClass::Oid)).equals((Alias::new("pg_attribute"), Alias::new("attrelid"))),
        )
        .inner_join(
            Alias::new("pg_collation"),
            Expr::col((Alias::new("pg_collation"), Alias::new("oid"))).equals((Alias::new("pg_attribute"), Alias::new("attcollation"))),
        )
        .and_where(Expr::col((PgClass::Table, PgClass::Relname)).eq(relation))
        .and_where(attribute("attname").eq(column))
        .and_where(attribute("attnum").gt(0))
        .and_where(attribute("attisdropped").not())
        .to_query_scalar()
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

    let anchored = explain(
        pool,
        Query::select()
            .column((Alias::new("e"), Entities::Id))
            .from_as(Entities::Table, Alias::new("e"))
            .and_where(Expr::col((Alias::new("e"), Entities::Id)).like(format!("{prefix}:seed:0001%")))
            .to_owned(),
    )
    .await;
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

    let unanchored = explain(
        pool,
        Query::select()
            .column((Alias::new("e"), Entities::Id))
            .from_as(Entities::Table, Alias::new("e"))
            .and_where(regex_match(Expr::col((Alias::new("e"), Entities::Id)), Expr::val("seed:01234")))
            .to_owned(),
    )
    .await;
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

    let ordered: Vec<String> = Query::select()
        .column(Entities::Id)
        .from(Entities::Table)
        .and_where(Expr::col(Entities::Id).like(format!("{}:seed:%", corpus.prefix())))
        .order_by(Entities::Id, Order::Asc)
        .limit(500)
        .to_query_scalar()
        .fetch_all(pool)
        .await
        .expect("ordered identifiers must be readable");

    let mut oracle = ordered.clone(); // clone: the oracle sorts an owned copy so the database order stays comparable.
    oracle.sort_by(|left, right| left.as_bytes().cmp(right.as_bytes()));
    assert_eq!(ordered, oracle, "ordering must be byte order, not a locale order");

    let exact = corpus.entity_id(7);
    let found: Option<String> = Query::select()
        .column(Entities::Id)
        .from(Entities::Table)
        .and_where(Expr::col(Entities::Id).eq(exact.as_str()))
        .to_query_scalar()
        .fetch_optional(pool)
        .await
        .expect("equality lookup must succeed");
    assert_eq!(found.as_deref(), Some(exact.as_str()), "equality must return the byte-identical identifier");

    scope.finish().await.expect("cleanup failed");
}
