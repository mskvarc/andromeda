//! Write-record release plans: releasing a write record seeks one index entry
//! per history chunk.
//!
//! A `write_records` row may be released only once no attribute history row,
//! no scope history row and no queued append intent references its write. Both
//! history relations are hypertables whose only index carrying `write_id` is
//! `uq_*_write (recorded_at, write_id, write_ordinal)`, so the release
//! correlates each history probe on the write's instant as well as its
//! identity. That is what lets every Temporal Evolution deletion (clause
//! 5.6.16) and every retention sweep seek one index entry per chunk instead of
//! scanning every chunk the history spans.

use crate::seeded_corpus::{PlannerPaths, SEED_PREFIX, explain_rolled_back};
use andromeda_postgresql_schema::{
    queue::{TemporalAppendIntents, WriteRecords},
    temporal::{AttributeHistory, ScopeHistory, TemporalAttributeStreams, TemporalEntities},
};
use andromeda_postgresql_seaquery::{
    aggregate::count_all,
    cast::cast,
    catalog_function::{generate_series, random_uuid},
    clock::now,
    command::Command,
    interval::make_interval_secs,
    literal::{int_literal, text_literal},
    maintenance::{MaintenanceSchema, analyze},
    pg_array::{any, array_literal},
    pg_operator::PgBinaryOperator,
    pg_type::PostgresType,
    statement::Statement,
};
use andromeda_test_harness::{
    dictionary_seed::{attribute_name_key, entity_type_name_key},
    serialized_environment::SerializedEnvironment,
};
use chrono::{DateTime, Utc};
use sea_query::{Alias, Expr, ExprTrait, IntoIden, Query};
use sqlx::PgPool;
use uuid::Uuid;

/// Weekly chunks the seeded history spans; the hypertables chunk on seven days
/// of `recorded_at`.
const SEEDED_WEEKS: i64 = 6;

/// Writes recorded per seeded week.
const WRITES_PER_WEEK: i64 = 2_000;

/// Writes whose release is planned.
const RELEASED_WRITES: i64 = 3;

/// Seconds in the seven days between two seeded weeks.
const WEEK_SECONDS: i64 = 7 * 24 * 60 * 60;

/// Type of the temporal Entity the seeded history belongs to.
const HISTORIC_TYPE: &str = "https://example.test/query-access-paths/Historic";

/// Expanded IRI of the attribute the seeded history records.
const RELEASE_ATTRIBUTE: &str = "https://example.test/query-access-paths/release-reading";

/// Write one attribute history row and one scope history row per write, spread
/// over [`SEEDED_WEEKS`] distinct weekly chunks, with the `write_records` row
/// each write owns, and analyze every relation the release reads.
///
/// Every history row carries the `recorded_at` of its write's record, which is
/// the invariant the committer keeps and the release correlation relies on.
async fn seed_released_history(pool: &PgPool, prefix: &str) {
    let type_key = entity_type_name_key(pool, HISTORIC_TYPE).await.expect("type dictionary seed must succeed");
    let entity_key = insert_release_entity(pool, prefix, type_key).await;
    let attribute_name_key = attribute_name_key(pool, RELEASE_ATTRIBUTE)
        .await
        .expect("attribute dictionary seed must succeed");
    let stream_key = insert_release_stream(pool, entity_key, attribute_name_key).await;
    insert_write_records(pool).await;
    insert_released_attribute_history(pool, stream_key).await;
    insert_released_scope_history(pool, entity_key).await;

    for relation in [
        WriteRecords::Table.into_iden(),
        AttributeHistory::Table.into_iden(),
        ScopeHistory::Table.into_iden(),
        TemporalAppendIntents::Table.into_iden(),
    ] {
        analyze(MaintenanceSchema::Public, relation)
            .to_query()
            .execute(pool)
            .await
            .expect("analyze must succeed");
    }
}

/// Column `column` of the write record aliased `w`.
fn record(column: WriteRecords) -> Expr {
    Expr::col((Alias::new("w"), column))
}

/// Insert the one temporal Entity of the corpus and return its key.
async fn insert_release_entity(pool: &PgPool, prefix: &str, type_key: i32) -> i64 {
    Query::insert()
        .into_table(TemporalEntities::Table)
        .columns([
            TemporalEntities::CreatedAt,
            TemporalEntities::ModifiedAt,
            TemporalEntities::Id,
            TemporalEntities::EntityTypeNameKeys,
        ])
        .values([
            now(),
            now(),
            cast(Expr::val(prefix), PostgresType::Text).concat_text(text_literal(":release")),
            array_literal([cast(Expr::val(type_key), PostgresType::Integer)]),
        ])
        .expect("one value per column")
        .returning_col(TemporalEntities::TemporalEntityKey)
        .to_query_scalar()
        .fetch_one(pool)
        .await
        .expect("temporal catalogue insert must succeed")
}

/// Insert the one stream of the attribute for the Entity and return its key.
async fn insert_release_stream(pool: &PgPool, entity_key: i64, attribute_name_key: i32) -> i64 {
    Query::insert()
        .into_table(TemporalAttributeStreams::Table)
        .columns([
            TemporalAttributeStreams::TemporalEntityKey,
            TemporalAttributeStreams::Id,
            TemporalAttributeStreams::AttributeNameKey,
        ])
        .select_from(
            Query::select()
                .column(TemporalEntities::TemporalEntityKey)
                .column(TemporalEntities::Id)
                .expr(Expr::val(attribute_name_key))
                .from(TemporalEntities::Table)
                .and_where(Expr::col(TemporalEntities::TemporalEntityKey).eq(entity_key))
                .to_owned(),
        )
        .expect("the projection matches the column list")
        .returning_col(TemporalAttributeStreams::StreamKey)
        .to_query_scalar()
        .fetch_one(pool)
        .await
        .expect("stream insert must succeed")
}

/// Insert the write records, spread over [`SEEDED_WEEKS`] weekly chunks.
async fn insert_write_records(pool: &PgPool) {
    let series = || Expr::col(Alias::new("g"));
    // 2026-01-05T00:00:00Z, a Monday.
    let first_week: DateTime<Utc> = DateTime::from_timestamp(1_767_571_200, 0).expect("the first week is a representable instant");
    let weeks_in = Expr::expr(series().modulo(SEEDED_WEEKS)).mul(WEEK_SECONDS);
    let minutes_in = Expr::expr(series().div(SEEDED_WEEKS)).mul(60);
    Query::insert()
        .into_table(WriteRecords::Table)
        .columns([WriteRecords::RecordedAt, WriteRecords::AcceptedAt, WriteRecords::WriteId])
        .select_from(
            Query::select()
                .expr(Expr::val(first_week).add(make_interval_secs(weeks_in)).add(make_interval_secs(minutes_in)))
                .expr(now())
                .expr(random_uuid())
                .from_function(generate_series(Expr::val(1), Expr::val(SEEDED_WEEKS * WRITES_PER_WEEK)), Alias::new("g"))
                .to_owned(),
        )
        .expect("the projection matches the column list")
        .to_query()
        .execute(pool)
        .await
        .expect("write record corpus must be writable");
}

/// Insert one attribute history row per write record.
async fn insert_released_attribute_history(pool: &PgPool, stream_key: i64) {
    Query::insert()
        .into_table(AttributeHistory::Table)
        .columns([
            AttributeHistory::RecordedAt,
            AttributeHistory::WriteId,
            AttributeHistory::StreamKey,
            AttributeHistory::WriteOrdinal,
            AttributeHistory::AttributeKind,
            AttributeHistory::ValueKind,
            AttributeHistory::ObservedAt,
            AttributeHistory::GeneratedInstanceId,
            AttributeHistory::ValueNumber,
        ])
        .select_from(
            Query::select()
                .expr(record(WriteRecords::RecordedAt))
                .expr(record(WriteRecords::WriteId))
                .expr(Expr::val(stream_key))
                .expr(Expr::val(0))
                .expr(Expr::val(1))
                .expr(Expr::val(1))
                .expr(record(WriteRecords::RecordedAt))
                .expr(random_uuid())
                .expr(Expr::val(1))
                .from_as(WriteRecords::Table, Alias::new("w"))
                .to_owned(),
        )
        .expect("the projection matches the column list")
        .to_query()
        .execute(pool)
        .await
        .expect("attribute history corpus must be writable");
}

/// Insert one scope history row per write record.
async fn insert_released_scope_history(pool: &PgPool, entity_key: i64) {
    Query::insert()
        .into_table(ScopeHistory::Table)
        .columns([
            ScopeHistory::RecordedAt,
            ScopeHistory::TemporalEntityKey,
            ScopeHistory::WriteId,
            ScopeHistory::WriteOrdinal,
            ScopeHistory::ValueKind,
            ScopeHistory::ObservedAt,
            ScopeHistory::ScopePath,
            ScopeHistory::VerbatimScopePath,
        ])
        .select_from(
            Query::select()
                .expr(record(WriteRecords::RecordedAt))
                .expr(Expr::val(entity_key))
                .expr(record(WriteRecords::WriteId))
                .expr(Expr::val(0))
                .expr(Expr::val(1))
                .expr(record(WriteRecords::RecordedAt))
                .expr(cast(text_literal("seed.release"), PostgresType::Ltree))
                .expr(text_literal("/seed/release"))
                .from_as(WriteRecords::Table, Alias::new("w"))
                .to_owned(),
        )
        .expect("the projection matches the column list")
        .to_query()
        .execute(pool)
        .await
        .expect("scope history corpus must be writable");
}

/// The number of distinct weekly chunks a history hypertable was split into.
async fn chunk_count(pool: &PgPool, hypertable: &str) -> i64 {
    Query::select()
        .expr(count_all())
        .from((Alias::new("timescaledb_information"), Alias::new("chunks")))
        .and_where(Expr::col(Alias::new("hypertable_name")).eq(hypertable))
        .to_query_scalar()
        .fetch_one(pool)
        .await
        .expect("the chunk catalogue must be readable")
}

/// The statement the release issues for a clause 5.6.16 deletion of `released`.
fn release_statement(released: Vec<Uuid>) -> sea_query::DeleteStatement {
    let history_probe = |table: &str| {
        Expr::exists(
            Query::select()
                .expr(int_literal(1))
                .from(Alias::new(table))
                .and_where(Expr::col((Alias::new(table), Alias::new("recorded_at"))).equals((WriteRecords::Table, WriteRecords::RecordedAt)))
                .and_where(Expr::col((Alias::new(table), Alias::new("write_id"))).equals((WriteRecords::Table, WriteRecords::WriteId)))
                .to_owned(),
        )
        .not()
    };
    Query::delete()
        .from_table(WriteRecords::Table)
        .and_where(history_probe("attribute_history"))
        .and_where(history_probe("scope_history"))
        .and_where(
            Expr::exists(
                Query::select()
                    .expr(int_literal(1))
                    .from(TemporalAppendIntents::Table)
                    .and_where(Expr::col((TemporalAppendIntents::Table, TemporalAppendIntents::WriteId)).equals((WriteRecords::Table, WriteRecords::WriteId)))
                    .to_owned(),
            )
            .not(),
        )
        .and_where(Expr::col((WriteRecords::Table, WriteRecords::WriteId)).eq(any(cast(Expr::val(released), PostgresType::UuidArray))))
        .to_owned()
}

#[tokio::test]
async fn write_record_release_seeks_the_history_write_index_instead_of_scanning_every_chunk() {
    let scope = SerializedEnvironment::from_environment_reset().await.expect("serialized test environment");
    let pool = scope.database_pool();
    seed_released_history(pool, SEED_PREFIX).await;
    assert!(
        chunk_count(pool, "attribute_history").await >= SEEDED_WEEKS,
        "the seeded attribute_history must span one chunk per week for the plan to mean anything"
    );

    let released: Vec<Uuid> = Query::select()
        .column(WriteRecords::WriteId)
        .from(WriteRecords::Table)
        .order_by(WriteRecords::WriteId, sea_query::Order::Asc)
        .limit(u64::try_from(RELEASED_WRITES).expect("fits"))
        .to_query_scalar()
        .fetch_all(pool)
        .await
        .expect("the released writes must be readable");

    // Every seeded record is protected, so the delete removes nothing; the
    // rollback keeps that true whatever the plan does.
    let plan = explain_rolled_back(pool, PlannerPaths::Unrestricted, release_statement(released)).await;
    for index in ["uq_attribute_history_write_identity", "uq_scope_history_write_identity"] {
        assert!(plan.contains(index), "the release must seek {index}: {plan}");
    }
    assert!(!plan.contains("Seq Scan on _hyper_"), "the release must not scan a history chunk: {plan}");

    scope.finish().await.expect("cleanup failed");
}
