//! Temporal access-path plans: an `idPattern` selection reaches an index.
//!
//! Stage 1 of a temporal query selects `temporal_entities` by the selector's
//! `idPattern` (clause 5.2.33), compiled to a POSIX `~` on the identifier. An
//! anchored pattern range-scans `uq_temporal_entities_id`; an unanchored one
//! has no anchor to range over and must reach the trigram index instead of
//! scanning the whole catalogue.

use crate::seeded_corpus::{SEED_PREFIX, explain};
use andromeda_postgresql_schema::current::Entities;
use andromeda_postgresql_seaquery::{
    cast::cast,
    catalog_function::{generate_series, lpad},
    clock::now,
    command::Command,
    literal::text_literal,
    maintenance::{MaintenanceSchema, analyze},
    operator::regex_match,
    pg_array::array_literal,
    pg_operator::PgBinaryOperator,
    pg_type::PostgresType,
    statement::Statement,
};
use andromeda_test_harness::{dictionary_seed::entity_type_name_key, serialized_environment::SerializedEnvironment};
use sea_query::{Alias, CaseStatement, Expr, ExprTrait, Query};
use sqlx::PgPool;

/// Temporal Entities written per seeded catalogue.
const SEEDED_TEMPORAL_ENTITIES: i64 = 20_000;

/// Type every seeded temporal Entity carries.
const TEMPORAL_TYPE: &str = "https://example.test/query-access-paths/Instrument";

/// Write the temporal catalogue and analyze it.
///
/// Every 1000th temporal Entity is a `sensor`, every other one a `device`, so
/// a pattern naming the sensors is selective enough that an index path is the
/// cheaper plan.
async fn seed_temporal_entities(pool: &PgPool, prefix: &str) {
    let type_key = entity_type_name_key(pool, TEMPORAL_TYPE).await.expect("type dictionary seed must succeed");
    let series = || Expr::col(Alias::new("g"));
    Query::insert()
        .into_table(Entities::Table)
        .columns([
            Entities::EvolutionCreatedAt,
            Entities::EvolutionModifiedAt,
            Entities::Id,
            Entities::EvolutionTypeNameKeys,
        ])
        .select_from(
            Query::select()
                .expr(now())
                .expr(now())
                .expr(
                    cast(Expr::val(prefix), PostgresType::Text)
                        .concat_text(text_literal(":"))
                        .concat_text(
                            CaseStatement::new()
                                .case(series().modulo(1000).eq(0), text_literal("sensor"))
                                .finally(text_literal("device"))
                                .into(),
                        )
                        .concat_text(text_literal(":"))
                        .concat_text(lpad(cast(series(), PostgresType::Text), Expr::val(6), text_literal("0"))),
                )
                .expr(array_literal([cast(Expr::val(type_key), PostgresType::Integer)]))
                .from_function(
                    generate_series(Expr::val(1), cast(Expr::val(SEEDED_TEMPORAL_ENTITIES), PostgresType::Bigint)),
                    Alias::new("g"),
                )
                .to_owned(),
        )
        .expect("the projection matches the column list")
        .to_query()
        .execute(pool)
        .await
        .expect("temporal catalogue corpus must be writable");

    analyze(MaintenanceSchema::Public, Entities::Table)
        .to_query()
        .execute(pool)
        .await
        .expect("analyze must succeed");
}

#[tokio::test]
async fn temporal_plans_an_unanchored_id_pattern_uses_the_trigram_index() {
    let scope = SerializedEnvironment::from_environment_reset().await.expect("serialized test environment");
    let pool = scope.database_pool();
    seed_temporal_entities(pool, SEED_PREFIX).await;

    // The stage 1 shape: the selector's pattern against the `e` alias the
    // temporal query selects `temporal_entities` under.
    let plan = explain(
        pool,
        Query::select()
            .columns([Entities::EntityKey, Entities::Id])
            .from_as(Entities::Table, Alias::new("e"))
            .and_where(regex_match(Expr::col((Alias::new("e"), Entities::Id)), Expr::val(".*sensor.*")))
            .to_owned(),
    )
    .await;
    assert!(
        plan.contains("idx_temporal_entities_id_trgm"),
        "an unanchored temporal idPattern must use the trigram index: {plan}"
    );

    scope.finish().await.expect("cleanup failed");
}
