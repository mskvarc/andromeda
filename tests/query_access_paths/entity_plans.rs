//! Verification-matrix row "Entity access-path plans".
//!
//! Each representative query shape is planned against a seeded, analyzed corpus
//! and must reach its intended index, and every typed-value index must exclude
//! the rows whose value column is null.

use crate::seeded_corpus::{COMMON_TYPE, RARE_TYPE, SEEDED_ENTITIES, explain, seed};
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use sqlx::PgPool;

/// One planned query shape: what it selects, the SQL that selects it, and the
/// index the schema exists to give it.
struct PlannedShape {
    /// Name reported when the plan misses its index.
    shape: &'static str,
    /// Statement handed to `EXPLAIN`.
    statement: String,
    /// Index the plan must use.
    index: &'static str,
}

#[tokio::test]
async fn entity_access_paths_reach_their_intended_index_for_every_query_shape() {
    let scope = TestEnvironment::from_suite("query-access-paths").await.expect("isolated test environment");
    let pool = scope.database_pool();
    let corpus = seed(pool, scope.id_prefix()).await;
    let reading = corpus.reading();
    let entrance = corpus.entrance();
    let prefix = corpus.prefix();

    let shapes = [
        PlannedShape {
            shape: "anchored id pattern",
            statement: format!("SELECT e.id FROM entities AS e WHERE e.id LIKE '{prefix}:seed:0001%'"),
            index: "entities_id_key",
        },
        PlannedShape {
            shape: "unanchored id pattern",
            statement: format!("SELECT e.id FROM entities AS e WHERE e.id ~ '{prefix}:seed:01234'"),
            index: "idx_entities_id_trgm",
        },
        PlannedShape {
            shape: "scope path",
            statement: "SELECT e.id FROM entities AS e WHERE e.scope_paths ~ 'seed.5'::lquery".to_owned(),
            index: "idx_entities_scope_paths",
        },
        PlannedShape {
            shape: "default-location geo",
            statement: "SELECT e.id FROM entities AS e \
                        WHERE ST_Intersects(e.location, ST_SetSRID(ST_MakePoint(13.005, 52.005), 4326))"
                .to_owned(),
            index: "idx_entities_location",
        },
        PlannedShape {
            shape: "arbitrary GeoProperty geo",
            statement: format!(
                "SELECT c.entity_key FROM current_attribute_values AS c \
                 WHERE c.attribute_name_id = {entrance} \
                   AND ST_Intersects(c.value_geo, ST_SetSRID(ST_MakePoint(13.005, 52.005), 4326))"
            ),
            index: "idx_cav_attr_geo",
        },
        PlannedShape {
            shape: "attribute existence",
            statement: format!(
                "SELECT e.id FROM entities AS e \
                 WHERE e.id LIKE '{prefix}:seed:00001%' \
                   AND EXISTS (SELECT 1 FROM current_attribute_values AS c \
                               WHERE c.entity_key = e.entity_key AND c.attribute_name_id = {reading})"
            ),
            index: "uq_current_attribute_values_grain",
        },
        PlannedShape {
            shape: "numeric q",
            statement: format!(
                "SELECT e.id FROM entities AS e \
                 WHERE EXISTS (SELECT 1 FROM current_attribute_values AS c \
                               WHERE c.entity_key = e.entity_key AND c.attribute_name_id = {reading} \
                                 AND c.value_number = 42::numeric)"
            ),
            index: "idx_cav_attr_number",
        },
    ];

    for shape in shapes {
        let plan = explain(pool, &shape.statement).await;
        assert!(plan.contains(shape.index), "the {} shape must use {}: {plan}", shape.shape, shape.index);
    }

    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn entity_access_path_for_a_type_filter_uses_the_types_gin_index() {
    let scope = TestEnvironment::from_suite("query-access-paths").await.expect("isolated test environment");
    let pool = scope.database_pool();
    let corpus = seed(pool, scope.id_prefix()).await;

    // The type filter is the one shape whose compiled form decides whether
    // `idx_entities_types` has a consumer at all: GIN `array_ops` serves `&&`,
    // `@>` and `<@`, and does not serve `= ANY(types)`. This is the containment
    // shape the type-selection rendering of clause 4.17 compiles to, with the
    // type carried in through the same single-element-array subquery.
    let compiled = format!("SELECT e.id FROM entities AS e WHERE e.types @> ARRAY(SELECT '{RARE_TYPE}'::TEXT)");
    let plan = explain(pool, &compiled).await;
    assert!(plan.contains("idx_entities_types"), "a type filter must reach the types GIN index: {plan}");

    // The same predicate must select the same Entities however it is planned,
    // so the residual oracle counts with the equality spelling the index cannot
    // answer and the two must agree.
    let selective: i64 = sqlx::query_scalar("SELECT count(*) FROM entities WHERE $1 = ANY(types) AND id LIKE $2::text || ':seed:%'")
        .bind(RARE_TYPE)
        .bind(corpus.prefix())
        .fetch_one(pool)
        .await
        .expect("type count must be readable");
    let contained: i64 = sqlx::query_scalar("SELECT count(*) FROM entities WHERE types @> ARRAY(SELECT $1::TEXT) AND id LIKE $2::text || ':seed:%'")
        .bind(RARE_TYPE)
        .bind(corpus.prefix())
        .fetch_one(pool)
        .await
        .expect("type count must be readable");
    assert_eq!(selective, contained, "containment and equality must select the same Entities");
    let common: i64 = sqlx::query_scalar("SELECT count(*) FROM entities WHERE types @> ARRAY(SELECT $1::TEXT) AND id LIKE $2::text || ':seed:%'")
        .bind(COMMON_TYPE)
        .bind(corpus.prefix())
        .fetch_one(pool)
        .await
        .expect("type count must be readable");
    assert_eq!(selective + common, SEEDED_ENTITIES);

    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn typed_value_indexes_exclude_rows_whose_value_column_is_null() {
    let scope = TestEnvironment::from_suite("query-access-paths").await.expect("isolated test environment");
    let pool = scope.database_pool();

    for (index, column) in [
        ("idx_cav_attr_number", "value_number"),
        ("idx_cav_attr_string", "value_string"),
        ("idx_cav_string_trgm", "value_string"),
        ("idx_cav_attr_bool", "value_bool"),
        ("idx_cav_attr_datetime", "value_datetime"),
        ("idx_cav_attr_uri", "value_uri"),
        ("idx_cav_attr_geo", "value_geo"),
        ("idx_cav_attr_entity_geo", "value_geo"),
    ] {
        let predicate = index_predicate(pool, index).await;
        assert_eq!(predicate, Some(format!("({column} IS NOT NULL)")), "{index} must be partial on {column}");
    }

    scope.finish().await.expect("cleanup failed");
}

/// Read the partial-index predicate of one index by name.
async fn index_predicate(pool: &PgPool, index: &str) -> Option<String> {
    sqlx::query_scalar(
        "SELECT pg_get_expr(i.indpred, i.indrelid) FROM pg_index AS i \
         JOIN pg_class AS c ON c.oid = i.indexrelid WHERE c.relname = $1",
    )
    .bind(index)
    .fetch_one(pool)
    .await
    .expect("the index must exist")
}
