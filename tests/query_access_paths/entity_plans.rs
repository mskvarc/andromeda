//! Verification-matrix row "Entity access-path plans".
//!
//! Each representative query shape is planned against a seeded, analyzed corpus
//! and must reach its intended index, every typed-value index must exclude the
//! rows whose value column is null, and the two relations a value-only write
//! must never touch an index of carry no index but their key.

use crate::seeded_corpus::{PlannerPaths, SEED_PREFIX, SEEDED_ENTITIES, explain, explain_rolled_back, seed, seed_essays};
use andromeda_test_harness::serialized_environment::SerializedEnvironment;
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
    let scope = SerializedEnvironment::from_environment_reset().await.expect("serialized test environment");
    let pool = scope.database_pool();
    let corpus = seed(pool, SEED_PREFIX).await;
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
    let scope = SerializedEnvironment::from_environment_reset().await.expect("serialized test environment");
    let pool = scope.database_pool();
    let corpus = seed(pool, SEED_PREFIX).await;

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
    let scope = SerializedEnvironment::from_environment_reset().await.expect("serialized test environment");
    let pool = scope.database_pool();

    for (index, column) in [
        ("idx_cav_attr_number", "value_number"),
        ("idx_cav_attr_string", "value_string"),
        ("idx_cav_attr_string_trgm", "value_string"),
        ("idx_cav_attr_bool", "value_bool"),
        ("idx_cav_attr_datetime", "value_datetime"),
        ("idx_cav_attr_uri", "value_uri"),
        ("idx_cav_attr_long_text", "value_long_text"),
        ("idx_cav_attr_long_text_trgm", "value_long_text"),
        ("idx_cav_attr_geo", "value_geo"),
        ("idx_cav_attr_entity_geo", "value_geo"),
    ] {
        let predicate = index_predicate(pool, index).await;
        assert_eq!(predicate, Some(format!("({column} IS NOT NULL)")), "{index} must be partial on {column}");
    }

    scope.finish().await.expect("cleanup failed");
}

/// A String past the 2048-octet bound is held in `value_long_text`, which no
/// B-tree can key, so the overflow column has its own pair of indexes: the
/// attribute-aware trigram GIN serves the clause 4.9 patterns, and the partial
/// B-tree on the attribute alone serves an ordering, whose value is then
/// filtered on the heap. The compiled pattern tests both string columns as one
/// disjunction, and each arm must reach its own trigram index.
///
/// The lone pattern names no attribute on purpose. The partial B-tree's
/// predicate and an attribute equality select the same rows, but the planner
/// multiplies their selectivities as if they were independent, so an
/// attribute-qualified pattern over one column underestimates the B-tree path
/// and filters every overflow row of the attribute on the heap instead. The
/// attribute-qualified disjunction, which is the shape the compiler emits,
/// reaches both trigram indexes at this population all the same.
#[tokio::test]
async fn long_string_access_paths_use_the_overflow_indexes() {
    let scope = SerializedEnvironment::from_environment_reset().await.expect("serialized test environment");
    let pool = scope.database_pool();
    let corpus = seed(pool, SEED_PREFIX).await;
    let essays = seed_essays(pool, &corpus).await;
    let attribute = essays.attribute();
    let probe = essays.probe();

    let pattern = explain(
        pool,
        &format!("SELECT c.entity_key FROM current_attribute_values AS c WHERE c.value_long_text ~ '{probe}'"),
    )
    .await;
    assert!(
        pattern.contains("idx_cav_attr_long_text_trgm"),
        "an unanchored pattern over long strings must use the overflow trigram index: {pattern}"
    );

    let disjunction = explain(
        pool,
        &format!(
            "SELECT c.entity_key FROM current_attribute_values AS c \
             WHERE c.attribute_name_id = {attribute} \
               AND (c.value_string ~ '{probe}' OR c.value_long_text ~ '{probe}')"
        ),
    )
    .await;
    assert!(
        disjunction.contains("BitmapOr") && disjunction.contains("idx_cav_attr_string_trgm") && disjunction.contains("idx_cav_attr_long_text_trgm"),
        "the compiled pattern must OR one bitmap per string column's trigram index: {disjunction}"
    );

    // The trailing space is the gap before the cost estimate, so the trigram
    // index, whose name extends this one, cannot satisfy the assertion.
    let ordering = explain_rolled_back(
        pool,
        PlannerPaths::WithoutSequentialScans,
        &format!(
            "SELECT c.entity_key FROM current_attribute_values AS c \
             WHERE c.attribute_name_id = {attribute} \
               AND (c.value_string > 'a' OR c.value_long_text > 'a')"
        ),
    )
    .await;
    assert!(
        ordering.contains("idx_cav_attr_long_text "),
        "an ordering over long strings must be driven from the overflow B-tree: {ordering}"
    );

    scope.finish().await.expect("cleanup failed");
}

/// The `attrs` axis is answered by the attribute keys the write path records
/// on the search row, so a term selects exactly the entities whose payload
/// carries the member: every seeded entity for the common attribute, one in a
/// thousand for the rare one.
#[tokio::test]
async fn attrs_terms_select_by_the_recorded_attribute_keys() {
    let scope = SerializedEnvironment::from_environment_reset().await.expect("serialized test environment");
    let pool = scope.database_pool();
    let corpus = seed(pool, SEED_PREFIX).await;

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
    let scope = SerializedEnvironment::from_environment_reset().await.expect("serialized test environment");
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
