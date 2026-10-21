//! Entity access-path plans: each query shape reaches its intended index.
//!
//! Each representative query shape is planned against a seeded, analyzed corpus
//! and must reach its intended index, every typed-value index must exclude the
//! rows whose value column is null, and no index covers the version token
//! every write sets.

use crate::seeded_corpus::{PlannerPaths, SEED_PREFIX, SEEDED_ENTITIES, explain, explain_rolled_back, owner_object, seed, seed_essays};
use andromeda_ngsi_ld_lexicon::time::rendering::instant_text;
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
    let cutoff = instant_text(Utc::now());

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
            statement: format!("SELECT s.entity_key FROM entity_search AS s WHERE s.attribute_name_keys @> ARRAY[{alarm}]"),
            index: "idx_entity_search_attribute_name_keys",
        },
        // The page shape the entity list issues: ordered by identifier, limited,
        // heads read by key. A key the planner could not estimate would make
        // it walk the identifier index and filter every row instead of probing
        // the GIN; the inlined key is what keeps the probe.
        PlannedShape {
            shape: "rare attrs page",
            statement: head_page(
                &format!("FROM entities AS e JOIN entity_search AS s USING (entity_key) WHERE s.attribute_name_keys @> ARRAY[{alarm}]"),
                0,
            ),
            index: "idx_entity_search_attribute_name_keys",
        },
        PlannedShape {
            shape: "attribute-expiry sweep",
            statement: format!(
                "SELECT e.entity_key, e.id, e.modified_at FROM entities AS e \
                 WHERE e.min_attribute_expires_at IS NOT NULL AND e.min_attribute_expires_at <= '{cutoff}'::timestamptz \
                 ORDER BY e.min_attribute_expires_at, e.id LIMIT 100"
            ),
            index: "idx_entities_min_attribute_expires_at",
        },
        PlannedShape {
            shape: "arbitrary GeoProperty geo",
            statement: format!(
                "SELECT c.entity_key FROM current_attributes AS c \
                 WHERE c.attribute_name_key = {entrance} \
                   AND ST_Intersects(c.value_geometry, ST_SetSRID(ST_MakePoint(13.005, 52.005), 4326))"
            ),
            index: "idx_ca_value_geometry",
        },
        PlannedShape {
            shape: "attribute existence",
            statement: format!(
                "SELECT e.id FROM entities AS e \
                 WHERE e.id LIKE '{prefix}:seed:00001%' \
                   AND EXISTS (SELECT 1 FROM current_attributes AS c \
                               WHERE c.entity_key = e.entity_key AND c.attribute_name_key = {reading})"
            ),
            index: "uq_current_attributes_grain",
        },
        PlannedShape {
            shape: "numeric q",
            statement: format!(
                "SELECT e.id FROM entities AS e \
                 WHERE EXISTS (SELECT 1 FROM current_attributes AS c \
                               WHERE c.entity_key = e.entity_key AND c.attribute_name_key = {reading} \
                                 AND c.value_number = 42::numeric)"
            ),
            index: "idx_ca_value_number",
        },
        PlannedShape {
            shape: "unanchored string pattern q",
            statement: format!(
                "SELECT c.entity_key FROM current_attributes AS c \
                 WHERE c.attribute_name_key = {city} AND c.value_string ~ 'ty-12'"
            ),
            index: "idx_ca_value_string_trgm",
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
    // `idx_entity_search_entity_type_name_keys` has a consumer at all: GIN `array_ops` serves
    // `&&`, `@>` and `<@`, and does not serve `= ANY(entity_type_name_keys)`. This is the
    // containment shape the type-selection rendering of clause 4.17 compiles
    // to, with the resolved dictionary key inlined as an array literal so the
    // planner estimates it from the column's element statistics.
    let compiled = format!(
        "SELECT s.entity_key FROM entity_search AS s WHERE s.entity_type_name_keys @> ARRAY[{}]",
        corpus.rare_type()
    );
    let plan = explain(pool, &compiled).await;
    assert!(
        plan.contains("idx_entity_search_entity_type_name_keys"),
        "a type filter must reach the entity_type_name_keys GIN index: {plan}"
    );

    // The same predicate must select the same Entities however it is planned,
    // so the residual oracle counts with the equality spelling the index cannot
    // answer and the two must agree.
    let selective: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM entities e JOIN entity_search s ON s.entity_key = e.entity_key WHERE $1 = ANY(s.entity_type_name_keys) AND e.id LIKE $2::text || ':seed:%'",
    )
    .bind(corpus.rare_type())
    .bind(corpus.prefix())
    .fetch_one(pool)
    .await
    .expect("type count must be readable");
    let contained: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM entities e JOIN entity_search s ON s.entity_key = e.entity_key WHERE s.entity_type_name_keys @> ARRAY(SELECT $1::integer) AND e.id LIKE $2::text || ':seed:%'",
    )
        .bind(corpus.rare_type())
        .bind(corpus.prefix())
        .fetch_one(pool)
        .await
        .expect("type count must be readable");
    assert_eq!(selective, contained, "containment and equality must select the same Entities");
    let common: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM entities e JOIN entity_search s ON s.entity_key = e.entity_key WHERE s.entity_type_name_keys @> ARRAY(SELECT $1::integer) AND e.id LIKE $2::text || ':seed:%'",
    )
        .bind(corpus.common_type())
        .bind(corpus.prefix())
        .fetch_one(pool)
        .await
        .expect("type count must be readable");
    assert_eq!(selective + common, SEEDED_ENTITIES);

    scope.finish().await.expect("cleanup failed");
}

/// Clauses 4.17 and 5.5.9: a page of one type walks that type's membership key
/// range in identifier order, so its cost follows the rows it skips and keeps
/// and never the Entities of other types whose identifiers sort ahead of it,
/// which a walk of `uq_entities_id` filtered by type would read. The
/// memberships select exactly the Entities the search row's types do.
#[tokio::test]
async fn a_type_page_walks_the_membership_key_in_identifier_order() {
    let scope = SerializedEnvironment::from_environment_reset().await.expect("serialized test environment");
    let pool = scope.database_pool();
    let corpus = seed(pool, SEED_PREFIX).await;

    for type_key in [corpus.rare_type(), corpus.common_type()] {
        let walk = format!("FROM (SELECT entity_key, id FROM entity_type_members WHERE entity_type_name_key = {type_key}) AS e");
        let plan = explain(pool, &head_page(&walk, 10)).await;
        assert!(plan.contains("pk_entity_type_members"), "the page walks the membership key: {plan}");
        assert!(!plan.contains("uq_entities_id"), "the page reads no other type's identifiers: {plan}");
        assert!(!plan.contains("Sort "), "the walk yields identifier order, so nothing is sorted: {plan}");

        let members: i64 = sqlx::query_scalar("SELECT count(*) FROM entity_type_members WHERE entity_type_name_key = $1")
            .bind(type_key)
            .fetch_one(pool)
            .await
            .expect("membership count must be readable");
        let searched: i64 = sqlx::query_scalar("SELECT count(*) FROM entity_search WHERE entity_type_name_keys @> ARRAY[$1::integer]")
            .bind(type_key)
            .fetch_one(pool)
            .await
            .expect("search count must be readable");
        assert_eq!(members, searched, "the memberships and the search rows select the same Entities");
    }

    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.7.2.4 lets any `location` instance match, so a `location`
/// geo-query is a semi-join over two arms: the hoisted default instance and
/// every instance row carrying a `datasetId`. Each arm must reach its own `GiST`
/// index, or the common single-instance query would lose the hoisted path. The
/// seeded `entrance` rows stand in
/// for the `location` instance rows: the arm reads them by the same key and
/// columns.
#[tokio::test]
async fn every_arm_of_a_location_geo_query_reaches_its_gist_index() {
    let scope = SerializedEnvironment::from_environment_reset().await.expect("serialized test environment");
    let pool = scope.database_pool();
    let corpus = seed(pool, SEED_PREFIX).await;
    let instances = corpus.entrance();

    let within = format!(
        "SELECT e.id FROM entities AS e JOIN entity_search AS s ON s.entity_key = e.entity_key \
         WHERE e.entity_key IN ( \
               SELECT geo_default.entity_key FROM entity_search AS geo_default \
                WHERE (geo_default.location_expires_at IS NULL OR geo_default.location_expires_at > NOW()) \
                  AND ST_Intersects(geo_default.location, ST_SetSRID(ST_MakePoint(13.005, 52.005), 4326)) \
               UNION ALL \
               SELECT c.entity_key FROM current_attributes AS c \
                WHERE c.attribute_name_key = {instances} AND c.dataset_id IS NOT NULL AND c.value_geometry IS NOT NULL \
                  AND (c.expires_at IS NULL OR c.expires_at > NOW()) \
                  AND ST_Intersects(c.value_geometry, ST_SetSRID(ST_MakePoint(13.005, 52.005), 4326)))"
    );
    let plan = explain(pool, &within).await;
    assert!(plan.contains("idx_entity_search_location"), "the hoisted arm must use its GiST index: {plan}");
    assert!(plan.contains("idx_ca_value_geometry"), "the instance arm must use its GiST index: {plan}");

    // `near` reads the hoisted `geography` twin, and prefilters the cast
    // instance geometry with a box the geometry GiST answers.
    let near = format!(
        "SELECT e.id FROM entities AS e JOIN entity_search AS s ON s.entity_key = e.entity_key \
         WHERE e.entity_key IN ( \
               SELECT geo_default.entity_key FROM entity_search AS geo_default \
                WHERE ST_DWithin(geo_default.location_geography, ST_SetSRID(ST_MakePoint(13.005, 52.005), 4326)::geography, 50.0) \
               UNION ALL \
               SELECT c.entity_key FROM current_attributes AS c \
                WHERE c.attribute_name_key = {instances} AND c.dataset_id IS NOT NULL AND c.value_geometry IS NOT NULL \
                  AND c.value_geometry && ST_MakeEnvelope(13.004, 52.004, 13.006, 52.006, 4326) \
                  AND ST_DWithin(c.value_geometry::geography, ST_SetSRID(ST_MakePoint(13.005, 52.005), 4326)::geography, 50.0))"
    );
    let plan = explain(pool, &near).await;
    assert!(
        plan.contains("idx_entity_search_location_geography"),
        "the hoisted arm must use its geography GiST index: {plan}"
    );
    assert!(
        plan.contains("idx_ca_value_geometry"),
        "the prefiltered instance arm must use its GiST index: {plan}"
    );

    scope.finish().await.expect("cleanup failed");
}

/// The value test of a compiled clause 4.9 string equality: every column the
/// target value can occupy, the structured column by containment.
fn string_equality(operand: &str) -> String {
    format!(
        "(c.value_string = '{operand}' OR c.value_uri = '{operand}' \
          OR c.value_object @> jsonb_build_array('{operand}'::text) \
          OR c.value_object @> jsonb_build_array(jsonb_build_object('@value', '{operand}'::text)))"
    )
}

/// The head statement a Query Entities page issues over `source` (its `FROM`
/// and `WHERE`, the identity row aliased `e`): the page of 100 entity keys is
/// cut in identifier order past `offset` rows, and each kept row's head is
/// looked up by key outside the window; its attribute rows are read by key
/// afterwards.
fn head_page(source: &str, offset: u32) -> String {
    format!(
        "SELECT stored.entity_key, stored.id, stored.created_at, stored.modified_at, stored.expires_at \
         FROM (SELECT e.entity_key, e.id AS sort_0 {source} ORDER BY sort_0 ASC LIMIT 100 OFFSET {offset}) AS page \
         JOIN LATERAL (SELECT pe.entity_key, pe.id, pe.created_at, pe.modified_at, pe.expires_at FROM entities AS pe \
                       WHERE pe.entity_key = page.entity_key LIMIT 1) AS stored ON TRUE \
         ORDER BY page.sort_0 ASC"
    )
}

/// The page shape a Query Entities `q` issues: ordered by identifier, limited,
/// with the probe the compiler emits.
fn q_page(probe: &str) -> String {
    head_page(
        &format!(
            "FROM entities AS e JOIN entity_search AS s ON s.entity_key = e.entity_key \
             WHERE EXISTS (SELECT 1 FROM current_attributes AS c \
                           WHERE (c.expires_at IS NULL OR c.expires_at > NOW()) AND {probe} \
                             AND c.entity_key = e.entity_key)"
        ),
        0,
    )
}

/// A page past an `OFFSET` reads the heads of the rows it keeps and no others:
/// the head lookup runs outside the window, once per kept row,
/// rather than once for every row the offset walks past. Whether the outer
/// statement re-sorts the page or the planner carries its order through, the
/// rows come back in the order the page was cut in (clause 4.23.1).
#[tokio::test]
async fn an_offset_page_reads_the_heads_of_its_kept_rows_only() {
    let scope = SerializedEnvironment::from_environment_reset().await.expect("serialized test environment");
    let pool = scope.database_pool();
    seed(pool, SEED_PREFIX).await;

    let plan = explain(pool, &head_page("FROM entities AS e", 5_000)).await;
    let lookup = plan
        .lines()
        .find(|line| line.contains("pk_entities"))
        .unwrap_or_else(|| panic!("the head is looked up by key: {plan}"));
    assert!(lookup.contains("loops=100)"), "one lookup per kept row, not per skipped row: {plan}");

    let ids: Vec<String> = sqlx::query_scalar(
        "SELECT page.id FROM (SELECT e.id, e.id AS sort_0 FROM entities AS e ORDER BY sort_0 ASC LIMIT 100 OFFSET 5000) AS page \
         ORDER BY page.sort_0 ASC",
    )
    .fetch_all(pool)
    .await
    .expect("the page must be readable");
    let mut sorted = ids.clone(); // clone: compared against the order the statement returned
    sorted.sort();
    assert_eq!(ids, sorted);
    assert_eq!(ids.len(), 100);

    scope.finish().await.expect("cleanup failed");
}

/// A clause 4.9 equality tests every column its target value can occupy, the
/// structured one by containment, so the probe is driven from the value indexes
/// only when every arm has one: a `BitmapOr` over the typed B-trees and the
/// `value_object` GIN. A disjunction of Query Terms shares that one probe, each
/// arm keeping its own attribute key, so it is driven the same way instead of
/// filtering every Entity in identifier order.
#[tokio::test]
async fn a_q_equality_and_a_q_disjunction_are_driven_from_every_value_index() {
    let scope = SerializedEnvironment::from_environment_reset().await.expect("serialized test environment");
    let pool = scope.database_pool();
    let corpus = seed(pool, SEED_PREFIX).await;
    let owner = corpus.owner();

    let first = owner_object(7);
    let second = owner_object(11);
    let shapes = [
        ("equality", format!("c.attribute_name_key = {owner} AND {}", string_equality(&first))),
        (
            "disjunction",
            format!(
                "((c.attribute_name_key = {owner} AND {}) OR (c.attribute_name_key = {owner} AND {}))",
                string_equality(&first),
                string_equality(&second)
            ),
        ),
    ];
    for (shape, probe) in shapes {
        let plan = explain(pool, &q_page(&probe)).await;
        for fragment in ["BitmapOr", "idx_ca_value_uri", "idx_ca_value_string ", "idx_ca_value_object"] {
            assert!(plan.contains(fragment), "the {shape} probe must use {fragment}: {plan}");
        }
        assert!(!plan.contains("uq_entities_id"), "the {shape} probe must not walk every Entity: {plan}");
    }

    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn typed_value_indexes_exclude_rows_whose_value_column_is_null() {
    let scope = SerializedEnvironment::from_environment_reset().await.expect("serialized test environment");
    let pool = scope.database_pool();

    for (index, column) in [
        ("idx_ca_value_number", "value_number"),
        ("idx_ca_value_string", "value_string"),
        ("idx_ca_value_string_trgm", "value_string"),
        ("idx_ca_value_boolean", "value_boolean"),
        ("idx_ca_value_date_time", "value_date_time"),
        ("idx_ca_value_uri", "value_uri"),
        ("idx_ca_value_long_text", "value_long_text"),
        ("idx_ca_value_long_text_trgm", "value_long_text"),
        ("idx_ca_value_long_uri", "value_long_uri"),
        ("idx_ca_value_object", "value_object"),
        ("idx_ca_value_geometry", "value_geometry"),
        ("idx_ca_entity_geometry", "value_geometry"),
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
        &format!("SELECT c.entity_key FROM current_attributes AS c WHERE c.value_long_text ~ '{probe}'"),
    )
    .await;
    assert!(
        pattern.contains("idx_ca_value_long_text_trgm"),
        "an unanchored pattern over long strings must use the overflow trigram index: {pattern}"
    );

    let disjunction = explain(
        pool,
        &format!(
            "SELECT c.entity_key FROM current_attributes AS c \
             WHERE c.attribute_name_key = {attribute} \
               AND (c.value_string ~ '{probe}' OR c.value_long_text ~ '{probe}')"
        ),
    )
    .await;
    assert!(
        disjunction.contains("BitmapOr") && disjunction.contains("idx_ca_value_string_trgm") && disjunction.contains("idx_ca_value_long_text_trgm"),
        "the compiled pattern must OR one bitmap per string column's trigram index: {disjunction}"
    );

    // The trailing space is the gap before the cost estimate, so the trigram
    // index, whose name extends this one, cannot satisfy the assertion.
    let ordering = explain_rolled_back(
        pool,
        PlannerPaths::WithoutSequentialScans,
        &format!(
            "SELECT c.entity_key FROM current_attributes AS c \
             WHERE c.attribute_name_key = {attribute} \
               AND (c.value_string > 'a' OR c.value_long_text > 'a')"
        ),
    )
    .await;
    assert!(
        ordering.contains("idx_ca_value_long_text "),
        "an ordering over long strings must be driven from the overflow B-tree: {ordering}"
    );

    scope.finish().await.expect("cleanup failed");
}

/// The `attrs` axis is answered by the attribute keys the write path records
/// on the search row, so a term selects exactly the entities whose rows carry
/// the member: every seeded entity for the common attribute, one in a
/// thousand for the rare one.
#[tokio::test]
async fn attrs_terms_select_by_the_recorded_attribute_keys() {
    let scope = SerializedEnvironment::from_environment_reset().await.expect("serialized test environment");
    let pool = scope.database_pool();
    let corpus = seed(pool, SEED_PREFIX).await;

    let count = |attribute: i32| {
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM entities e JOIN entity_search s ON s.entity_key = e.entity_key \
             WHERE s.attribute_name_keys @> ARRAY(SELECT $1::integer) AND e.id LIKE $2::text || ':seed:%'",
        )
        .bind(attribute)
        .bind(corpus.prefix())
        .fetch_one(pool)
    };
    assert_eq!(count(corpus.city()).await.expect("attrs count must be readable"), SEEDED_ENTITIES);
    assert_eq!(count(corpus.alarm()).await.expect("attrs count must be readable"), SEEDED_ENTITIES / 1000);

    scope.finish().await.expect("cleanup failed");
}

/// Every write sets `entities.modified_at`, so no index of the identity or
/// search row may cover it: the update stays heap-only and a value-only write
/// can never touch a search index.
#[tokio::test]
async fn no_index_covers_the_version_token() {
    let scope = SerializedEnvironment::from_environment_reset().await.expect("serialized test environment");
    let pool = scope.database_pool();

    let indexed_definitions: Vec<String> =
        sqlx::query_scalar("SELECT indexdef FROM pg_indexes WHERE schemaname = 'public' AND tablename IN ('entities', 'entity_search')")
            .fetch_all(pool)
            .await
            .expect("the index catalogue must be readable");
    for definition in &indexed_definitions {
        assert!(!definition.contains("modified_at"), "{definition}");
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
