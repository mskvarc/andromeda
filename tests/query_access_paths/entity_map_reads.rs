//! Entity Map read plans: a page that references a map walks the map's key in
//! production order.
//!
//! Every follow-up page of a distributed query, and every retrieval of the map
//! itself, reads the whole `entityMap` in the order the originating query
//! produced it (clauses 5.2.39 and 5.5.14). `entity_map_entries` is keyed on
//! `(entity_map_key, position)` so that read is one ordered index walk with no
//! sort, and the prune a page issues for entities that stopped matching
//! (clause 4.3.6.7) is answered by the `(entity_map_key, entity_id)` index.

use crate::seeded_corpus::{PlannerPaths, SEED_PREFIX, explain, explain_rolled_back};
use andromeda_test_harness::serialized_environment::SerializedEnvironment;
use pretty_assertions::assert_eq;
use sqlx::PgPool;
use std::iter::{once, repeat_n};

/// Maps the corpus holds.
const SEEDED_MAPS: i32 = 40;

/// Entries per seeded map.
const ENTRIES_PER_MAP: i32 = 5_000;

/// Entities the planned prune sheds.
const SHED_ENTITIES: i32 = 100;

/// The identifier of the map the plans read, halfway through the corpus.
fn probed_map() -> String {
    format!("{SEED_PREFIX}:EntityMap:{}", SEEDED_MAPS / 2)
}

/// Seed [`SEEDED_MAPS`] live maps of [`ENTRIES_PER_MAP`] entries each and
/// analyze both relations.
///
/// Entity identifiers are digests of their position, so identifier order is
/// unrelated to production order, as it is for a real query's result. One
/// entity in five is attributed to a registration, the rest to `@none`.
async fn seed_maps(pool: &PgPool, prefix: &str) {
    sqlx::query("INSERT INTO entity_maps (expires_at, id) SELECT now() + interval '1 hour', $1 || ':EntityMap:' || m FROM generate_series(1, $2) AS m")
        .bind(prefix)
        .bind(SEEDED_MAPS)
        .execute(pool)
        .await
        .expect("the map control records must be seeded");
    sqlx::query(
        "INSERT INTO entity_map_entries (entity_map_key, position, entity_id, source) \
         SELECT em.entity_map_key, n - 1, $1 || ':Vehicle:' || md5(em.entity_map_key || ':' || n), \
                CASE WHEN n % 5 = 0 THEN $1 || ':ContextSourceRegistration:' || (n % 7) ELSE '@none' END \
         FROM entity_maps AS em CROSS JOIN generate_series(1, $2) AS n \
         WHERE em.id LIKE $1 || '%' \
         ORDER BY em.entity_map_key, n",
    )
    .bind(prefix)
    .bind(ENTRIES_PER_MAP)
    .execute(pool)
    .await
    .expect("the map entries must be seeded");
    for statement in ["ANALYZE entity_maps", "ANALYZE entity_map_entries"] {
        sqlx::query(statement).execute(pool).await.expect("analyze must succeed");
    }
}

/// The scalar subquery the store resolves a map's identifier with.
fn entity_map_key_of(id: &str) -> String {
    format!(r#"(SELECT "entity_maps"."entity_map_key" FROM "entity_maps" WHERE "entity_maps"."id" = '{id}')"#)
}

#[tokio::test]
async fn entity_map_read_walks_the_position_key_without_a_sort() {
    let scope = SerializedEnvironment::from_environment_reset().await.expect("serialized test environment");
    let pool = scope.database_pool();
    seed_maps(pool, SEED_PREFIX).await;
    let key = entity_map_key_of(&probed_map());

    // The whole-map read exactly as the store renders it, with the map's
    // identifier inlined.
    let read = format!(r#"SELECT "entity_id", "source" FROM "entity_map_entries" WHERE "entity_map_key" = {key} ORDER BY "position" ASC"#);
    let plan = explain(pool, &read).await;
    assert!(
        plan.contains("Index Scan using pk_entity_map_entries on entity_map_entries"),
        "the read must walk the position key: {plan}"
    );
    assert!(!plan.contains("Sort"), "the key already gives production order, so nothing is sorted: {plan}");

    let positions: Vec<i32> = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        r#"SELECT "position" FROM "entity_map_entries" WHERE "entity_map_key" = {key} ORDER BY "position" ASC"#
    )))
    .fetch_all(pool)
    .await
    .expect("the positions must be readable");
    assert_eq!(
        positions,
        (0..ENTRIES_PER_MAP).collect::<Vec<i32>>(),
        "the read returns every entry of the one map, in production order"
    );

    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn entity_map_prune_seeks_the_entity_index_and_removes_only_the_named_entities() {
    let scope = SerializedEnvironment::from_environment_reset().await.expect("serialized test environment");
    let pool = scope.database_pool();
    seed_maps(pool, SEED_PREFIX).await;
    let key = entity_map_key_of(&probed_map());

    let shed: Vec<String> = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        r#"SELECT "entity_id" FROM "entity_map_entries" WHERE "entity_map_key" = {key} ORDER BY "position" ASC LIMIT {SHED_ENTITIES}"#
    )))
    .fetch_all(pool)
    .await
    .expect("the shed entities must be readable");
    let shed = shed.iter().map(|entity_id| format!("'{entity_id}'")).collect::<Vec<String>>().join(", ");

    // The delete the store issues for a page that shed entities, with its bound
    // values inlined so the plan reads them as constants.
    let prune = format!(
        r#"DELETE FROM "entity_map_entries" WHERE "entity_map_key" = (SELECT "entity_maps"."entity_map_key" FROM "entity_maps" WHERE "entity_maps"."id" = '{}' AND "entity_maps"."expires_at" > now()) AND "entity_id" = ANY(ARRAY[{shed}])"#,
        probed_map()
    );
    let plan = explain_rolled_back(pool, PlannerPaths::Unrestricted, &prune).await;
    assert!(
        plan.contains("idx_entity_map_entries_entity_id"),
        "the prune must seek the entity index: {plan}"
    );
    assert!(!plan.contains("Seq Scan on entity_map_entries"), "the prune must not scan the entries: {plan}");

    let removed = sqlx::query(sqlx::AssertSqlSafe(prune))
        .execute(pool)
        .await
        .expect("the prune must run")
        .rows_affected();
    assert_eq!(removed, u64::try_from(SHED_ENTITIES).expect("fits"), "every pair of the named entities goes");
    let remaining: Vec<i64> = sqlx::query_scalar(
        "SELECT count(*) FROM entity_maps AS em JOIN entity_map_entries AS e USING (entity_map_key) \
         WHERE em.id LIKE $1 || '%' GROUP BY em.entity_map_key ORDER BY count(*)",
    )
    .bind(SEED_PREFIX)
    .fetch_all(pool)
    .await
    .expect("the remaining entries must be countable");
    let expected: Vec<i64> = once(i64::from(ENTRIES_PER_MAP - SHED_ENTITIES))
        .chain(repeat_n(i64::from(ENTRIES_PER_MAP), usize::try_from(SEEDED_MAPS - 1).expect("fits")))
        .collect();
    assert_eq!(
        remaining, expected,
        "the pruned map lost exactly the named entities and every other map kept all of its own"
    );

    scope.finish().await.expect("cleanup failed");
}
