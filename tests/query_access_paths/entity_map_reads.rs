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
use andromeda_postgresql_schema::entity_map::{EntityMapEntries, EntityMaps};
use andromeda_postgresql_seaquery::{
    aggregate::count_all,
    cast::cast,
    catalog_function::{generate_series, md5},
    clock::now,
    command::Command,
    interval::make_interval_secs,
    literal::text_literal,
    maintenance::{MaintenanceSchema, analyze},
    pg_array::any,
    pg_operator::PgBinaryOperator,
    pg_type::PostgresType,
    scalar_subquery::scalar_subquery,
    statement::Statement,
};
use andromeda_test_harness::serialized_environment::SerializedEnvironment;
use pretty_assertions::assert_eq;
use sea_query::{Alias, CaseStatement, Expr, ExprTrait, IntoIden, Order, Query, SelectStatement, SimpleExpr};
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
    let series = |name: &str| Expr::col(Alias::new(name));
    Query::insert()
        .into_table(EntityMaps::Table)
        .columns([EntityMaps::ExpiresAt, EntityMaps::Id])
        .select_from(
            Query::select()
                .expr(Expr::expr(now()).add(make_interval_secs(Expr::val(3600.0))))
                .expr(
                    cast(Expr::val(prefix), PostgresType::Text)
                        .concat_text(text_literal(":EntityMap:"))
                        .concat_text(series("m")),
                )
                .from_function(generate_series(Expr::val(1), Expr::val(SEEDED_MAPS)), Alias::new("m"))
                .to_owned(),
        )
        .expect("the projection matches the column list")
        .to_query()
        .execute(pool)
        .await
        .expect("the map control records must be seeded");
    let map_key = || Expr::col((Alias::new("em"), EntityMaps::EntityMapKey));
    let registration = cast(Expr::val(prefix), PostgresType::Text)
        .concat_text(text_literal(":ContextSourceRegistration:"))
        .concat_text(Expr::expr(series("n").modulo(7)));
    Query::insert()
        .into_table(EntityMapEntries::Table)
        .columns([
            EntityMapEntries::EntityMapKey,
            EntityMapEntries::Position,
            EntityMapEntries::EntityId,
            EntityMapEntries::Source,
        ])
        .select_from(
            Query::select()
                .expr(map_key())
                .expr(Expr::expr(series("n")).sub(1))
                .expr(
                    cast(Expr::val(prefix), PostgresType::Text)
                        .concat_text(text_literal(":Vehicle:"))
                        .concat_text(md5(map_key().concat_text(text_literal(":")).concat_text(series("n")))),
                )
                .expr(
                    CaseStatement::new()
                        .case(series("n").modulo(5).eq(0), registration)
                        .finally(text_literal("@none")),
                )
                .from_as(EntityMaps::Table, Alias::new("em"))
                .from_function(generate_series(Expr::val(1), Expr::val(ENTRIES_PER_MAP)), Alias::new("n"))
                .and_where(Expr::col((Alias::new("em"), EntityMaps::Id)).like(format!("{prefix}%")))
                .order_by((Alias::new("em"), EntityMaps::EntityMapKey), Order::Asc)
                .order_by(Alias::new("n"), Order::Asc)
                .to_owned(),
        )
        .expect("the projection matches the column list")
        .to_query()
        .execute(pool)
        .await
        .expect("the map entries must be seeded");
    for relation in [EntityMaps::Table.into_iden(), EntityMapEntries::Table.into_iden()] {
        analyze(MaintenanceSchema::Public, relation)
            .to_query()
            .execute(pool)
            .await
            .expect("analyze must succeed");
    }
}

/// The scalar subquery the store resolves a map's identifier with.
fn entity_map_key_of(id: &str) -> SimpleExpr {
    scalar_subquery(
        Query::select()
            .column((EntityMaps::Table, EntityMaps::EntityMapKey))
            .from(EntityMaps::Table)
            .and_where(Expr::col((EntityMaps::Table, EntityMaps::Id)).eq(id))
            .to_owned(),
    )
}

/// The whole-map read exactly as the store renders it, for the map `id`.
fn map_read(id: &str) -> SelectStatement {
    Query::select()
        .from(EntityMapEntries::Table)
        .and_where(Expr::col(EntityMapEntries::EntityMapKey).eq(entity_map_key_of(id)))
        .order_by(EntityMapEntries::Position, Order::Asc)
        .to_owned()
}

/// The delete the store issues for a page of the map `id` that shed `shed`.
fn prune(id: &str, shed: Vec<String>) -> sea_query::DeleteStatement {
    Query::delete()
        .from_table(EntityMapEntries::Table)
        .and_where(
            Expr::col(EntityMapEntries::EntityMapKey).eq(scalar_subquery(
                Query::select()
                    .column((EntityMaps::Table, EntityMaps::EntityMapKey))
                    .from(EntityMaps::Table)
                    .and_where(Expr::col((EntityMaps::Table, EntityMaps::Id)).eq(id))
                    .and_where(Expr::col((EntityMaps::Table, EntityMaps::ExpiresAt)).gt(now()))
                    .to_owned(),
            )),
        )
        .and_where(Expr::col(EntityMapEntries::EntityId).eq(any(Expr::val(shed))))
        .to_owned()
}

#[tokio::test]
async fn entity_map_read_walks_the_position_key_without_a_sort() {
    let scope = SerializedEnvironment::from_environment_reset().await.expect("serialized test environment");
    let pool = scope.database_pool();
    seed_maps(pool, SEED_PREFIX).await;

    // The whole-map read exactly as the store renders it.
    let plan = explain(
        pool,
        map_read(&probed_map())
            .columns([EntityMapEntries::EntityId, EntityMapEntries::Source])
            .to_owned(),
    )
    .await;
    assert!(
        plan.contains("Index Scan using pk_entity_map_entries on entity_map_entries"),
        "the read must walk the position key: {plan}"
    );
    assert!(!plan.contains("Sort"), "the key already gives production order, so nothing is sorted: {plan}");

    let positions: Vec<i32> = map_read(&probed_map())
        .column(EntityMapEntries::Position)
        .to_query_scalar()
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

    let shed: Vec<String> = map_read(&probed_map())
        .column(EntityMapEntries::EntityId)
        .limit(u64::try_from(SHED_ENTITIES).expect("fits"))
        .to_query_scalar()
        .fetch_all(pool)
        .await
        .expect("the shed entities must be readable");

    // The delete the store issues for a page that shed entities.
    let plan = explain_rolled_back(pool, PlannerPaths::Unrestricted, prune(&probed_map(), shed.clone())).await; // clone: the plan and the run each own the shed set.
    assert!(
        plan.contains("idx_entity_map_entries_entity_id"),
        "the prune must seek the entity index: {plan}"
    );
    assert!(!plan.contains("Seq Scan on entity_map_entries"), "the prune must not scan the entries: {plan}");

    let removed = prune(&probed_map(), shed)
        .to_query()
        .execute(pool)
        .await
        .expect("the prune must run")
        .rows_affected();
    assert_eq!(removed, u64::try_from(SHED_ENTITIES).expect("fits"), "every pair of the named entities goes");
    let remaining: Vec<i64> = Query::select()
        .expr(count_all())
        .from(EntityMaps::Table)
        .inner_join(
            EntityMapEntries::Table,
            Expr::col((EntityMapEntries::Table, EntityMapEntries::EntityMapKey)).equals((EntityMaps::Table, EntityMaps::EntityMapKey)),
        )
        .and_where(Expr::col((EntityMaps::Table, EntityMaps::Id)).like(format!("{SEED_PREFIX}%")))
        .group_by_col((EntityMaps::Table, EntityMaps::EntityMapKey))
        .order_by_expr(count_all(), Order::Asc)
        .to_query_scalar()
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
