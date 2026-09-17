//! Verification-matrix row "Entity access-path plans".
//!
//! Each representative query shape is planned against a seeded, analyzed corpus
//! and must reach its intended index, every typed-value index must exclude the
//! rows whose value column is null, and the two relations a value-only write
//! must never touch an index of carry no index but their key.

use crate::seeded_corpus::{SEEDED_ENTITIES, explain, seed};
use andromeda_test_harness::environment::TestEnvironment;
use chrono::Utc;
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
    let alarm = corpus.alarm();
    let city = corpus.city();
    let prefix = corpus.prefix();
    let cutoff = Utc::now().to_rfc3339();

    let shapes = [
        PlannedShape {
            shape: "anchored id pattern",
            statement: format!("SELECT e.id FROM entities AS e WHERE e.id LIKE '{prefix}:seed:0001%'"),
            index: "uq_entities_id",
        },
        PlannedShape {
            shape: "unanchored id pattern",
            statement: format!("SELECT e.id FROM entities AS e WHERE e.id ~ '{prefix}:seed:01234'"),
            index: "idx_entities_id_trgm",
        },
        PlannedShape {
            shape: "scope path",
            statement: "SELECT s.entity_key FROM entity_search AS s WHERE s.scope_paths ~ 'seed.5'::lquery".to_owned(),
            index: "idx_entity_search_scope_paths",
        },
        PlannedShape {
            shape: "default-location geo",
            statement: "SELECT s.entity_key FROM entity_search AS s \
                        WHERE ST_Intersects(s.location, ST_SetSRID(ST_MakePoint(13.005, 52.005), 4326))"
                .to_owned(),
            index: "idx_entity_search_location",
        },
        PlannedShape {
            shape: "rare attrs term",
            statement: format!("SELECT s.entity_key FROM entity_search AS s WHERE s.attribute_name_ids @> ARRAY[{alarm}]"),
            index: "idx_entity_search_attribute_name_ids",
        },
        // The page shape the entity list issues: ordered by identifier, limited,
        // payload read by key. A key the planner could not estimate would make
        // it walk the identifier index and filter every row instead of probing
        // the GIN; the inlined key is what keeps the probe.
        PlannedShape {
            shape: "rare attrs page",
            statement: format!(
                "SELECT (SELECT d.payload FROM entity_documents AS d WHERE d.entity_key = e.entity_key) AS payload \
                 FROM entities AS e JOIN entity_search AS s ON s.entity_key = e.entity_key \
                 WHERE s.attribute_name_ids @> ARRAY[{alarm}] ORDER BY e.id LIMIT 100"
            ),
            index: "idx_entity_search_attribute_name_ids",
        },
        PlannedShape {
            shape: "attribute-expiry sweep",
            statement: format!(
                "SELECT e.id, d.payload, d.modified_at FROM entities AS e \
                 JOIN entity_documents AS d ON d.entity_key = e.entity_key \
                 WHERE e.min_attribute_expires_at IS NOT NULL AND e.min_attribute_expires_at <= '{cutoff}'::timestamptz \
                 ORDER BY e.min_attribute_expires_at, e.id LIMIT 100"
            ),
            index: "idx_entities_min_attribute_expires_at",
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
        PlannedShape {
            shape: "unanchored string pattern q",
            statement: format!(
                "SELECT c.entity_key FROM current_attribute_values AS c \
                 WHERE c.attribute_name_id = {city} AND c.value_string ~ 'ty-12'"
            ),
            index: "idx_cav_attr_string_trgm",
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
    // `idx_entity_search_type_ids` has a consumer at all: GIN `array_ops` serves
    // `&&`, `@>` and `<@`, and does not serve `= ANY(type_ids)`. This is the
    // containment shape the type-selection rendering of clause 4.17 compiles
    // to, with the resolved dictionary key inlined as an array literal so the
    // planner estimates it from the column's element statistics.
    let compiled = format!("SELECT s.entity_key FROM entity_search AS s WHERE s.type_ids @> ARRAY[{}]", corpus.rare_type());
    let plan = explain(pool, &compiled).await;
    assert!(
        plan.contains("idx_entity_search_type_ids"),
        "a type filter must reach the type_ids GIN index: {plan}"
    );

    // The same predicate must select the same Entities however it is planned,
    // so the residual oracle counts with the equality spelling the index cannot
    // answer and the two must agree.
    let selective: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM entities e JOIN entity_search s ON s.entity_key = e.entity_key WHERE $1 = ANY(s.type_ids) AND e.id LIKE $2::text || ':seed:%'",
    )
    .bind(corpus.rare_type())
    .bind(corpus.prefix())
    .fetch_one(pool)
    .await
    .expect("type count must be readable");
    let contained: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM entities e JOIN entity_search s ON s.entity_key = e.entity_key WHERE s.type_ids @> ARRAY(SELECT $1::integer) AND e.id LIKE $2::text || ':seed:%'",
    )
        .bind(corpus.rare_type())
        .bind(corpus.prefix())
        .fetch_one(pool)
        .await
        .expect("type count must be readable");
    assert_eq!(selective, contained, "containment and equality must select the same Entities");
    let common: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM entities e JOIN entity_search s ON s.entity_key = e.entity_key WHERE s.type_ids @> ARRAY(SELECT $1::integer) AND e.id LIKE $2::text || ':seed:%'",
    )
        .bind(corpus.common_type())
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
        ("idx_cav_attr_string_trgm", "value_string"),
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

/// The `attrs` axis is answered by the attribute keys the write path records
/// on the search row, so a term selects exactly the entities whose payload
/// carries the member: every seeded entity for the common attribute, one in a
/// thousand for the rare one.
#[tokio::test]
async fn attrs_terms_select_by_the_recorded_attribute_keys() {
    let scope = TestEnvironment::from_suite("query-access-paths").await.expect("isolated test environment");
    let pool = scope.database_pool();
    let corpus = seed(pool, scope.id_prefix()).await;

    let count = |attribute: i32| {
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM entities e JOIN entity_search s ON s.entity_key = e.entity_key \
             WHERE s.attribute_name_ids @> ARRAY(SELECT $1::integer) AND e.id LIKE $2::text || ':seed:%'",
        )
        .bind(attribute)
        .bind(corpus.prefix())
        .fetch_one(pool)
    };
    assert_eq!(count(corpus.city()).await.expect("attrs count must be readable"), SEEDED_ENTITIES);
    assert_eq!(count(corpus.alarm()).await.expect("attrs count must be readable"), SEEDED_ENTITIES / 1000);

    scope.finish().await.expect("cleanup failed");
}

/// The document row is rewritten by every attribute write, so it must carry no
/// index but its key; the identity row carries no payload or version-token
/// index either, so a value-only write can never touch a search index.
#[tokio::test]
async fn the_document_row_carries_no_index_but_its_key_and_no_payload_column_is_indexed() {
    let scope = TestEnvironment::from_suite("query-access-paths").await.expect("isolated test environment");
    let pool = scope.database_pool();

    let document_indexes: Vec<String> =
        sqlx::query_scalar("SELECT indexname FROM pg_indexes WHERE schemaname = 'public' AND tablename = 'entity_documents' ORDER BY 1")
            .fetch_all(pool)
            .await
            .expect("the index catalogue must be readable");
    assert_eq!(document_indexes, vec!["pk_entity_documents".to_owned()]);

    let indexed_definitions: Vec<String> =
        sqlx::query_scalar("SELECT indexdef FROM pg_indexes WHERE schemaname = 'public' AND tablename IN ('entities', 'entity_documents', 'entity_search')")
            .fetch_all(pool)
            .await
            .expect("the index catalogue must be readable");
    for definition in &indexed_definitions {
        assert!(!definition.contains("payload") && !definition.contains("modified_at"), "{definition}");
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
