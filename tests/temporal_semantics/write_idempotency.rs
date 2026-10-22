//! Write idempotency.
//!
//! One atomic write owns a `write_records` row that persists the instant its
//! consequences were recorded at. A replay reuses that instant, so it collides
//! with the rows it would otherwise duplicate on the hypertable's uniqueness
//! key. The record is released only once nothing it protects is left, which
//! Temporal Evolution deletion (clause 5.6.16) is one of the two callers of.

use crate::{
    clean_database,
    client,
    create_entity,
    database,
    delete_temporal_entity,
    stored_history::{StoredHistoryRow, orphaned_temporal_rows, stored_history},
    unique_id,
};
use andromeda_postgresql_schema::{
    queue::WriteRecords,
    temporal::{AttributeHistory, TemporalAttributeStreams},
};
use andromeda_postgresql_seaquery::{aggregate::count_all, pg_array::any, statement::Statement};
use chrono::{DateTime, TimeDelta, Utc};
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use sea_query::{Alias, Expr, ExprTrait, InsertStatement, OnConflict, Query};
use serde_json::json;
use serial_test::serial;
use sqlx::{PgPool, Row, error::DatabaseError};
use std::collections::BTreeSet;
use uuid::Uuid;

/// Count the idempotency records among a named set of write identities.
async fn write_records_present(pool: &PgPool, write_ids: &[Uuid]) -> i64 {
    Query::select()
        .expr_as(count_all(), Alias::new("records"))
        .from(WriteRecords::Table)
        .and_where(Expr::col(WriteRecords::WriteId).eq(any(Expr::val(write_ids.to_vec()))))
        .to_query()
        .fetch_one(pool)
        .await
        .expect("idempotency record count")
        .try_get("records")
        .expect("the records column decodes")
}

/// Read the instant one idempotency record persists.
async fn recorded_instant(pool: &PgPool, write_id: Uuid) -> DateTime<Utc> {
    Query::select()
        .column(WriteRecords::RecordedAt)
        .from(WriteRecords::Table)
        .and_where(Expr::col(WriteRecords::WriteId).eq(write_id))
        .to_query()
        .fetch_one(pool)
        .await
        .expect("the idempotency record of a committed write")
        .try_get("recorded_at")
        .expect("the recorded_at column decodes")
}

/// A replayed write reuses its persisted instant, so the hypertable's
/// uniqueness key rejects the repeat rather than storing history twice.
#[tokio::test]
#[serial]
async fn temporal_semantics_write_idempotency_replays_collide_instead_of_duplicating() {
    let _serialized_environment = clean_database().await;
    let http = client();
    let pool = database().await;
    let entity_id = unique_id("urn:ngsi-ld:Vehicle:idempotency_replay");

    let created = create_entity(
        &http,
        &json!({ "id": entity_id, "type": "Vehicle", "speed": { "type": "Property", "value": 55 } }),
    )
    .await;
    assert_eq!(created, StatusCode::CREATED);

    let rows = stored_history(&pool, &entity_id).await;
    assert_eq!(rows.len(), 1, "the fixture records one instance");
    let original = rows[0].clone();

    // The write's own record persists the instant every consequence carries.
    assert_eq!(
        recorded_instant(&pool, original.write_id).await,
        original.recorded_at,
        "the idempotency record persists the instant the write recorded at"
    );

    // A replay that reuses that identity and that instant collides.
    let replayed = replay_history_row(&original).to_query().execute(&pool).await;
    let violation = replayed.expect_err("a replayed consequence must not be stored twice");
    assert_eq!(
        violation.as_database_error().and_then(DatabaseError::code).as_deref(),
        Some("23505"),
        "the collision is a uniqueness violation on (recorded_at, write_id, write_ordinal)"
    );

    // Claiming the same write again is recognised rather than recorded twice,
    // and it does not move the persisted instant a replay has to reuse.
    let reclaimed = claim_write_record(original.write_id, original.recorded_at + TimeDelta::hours(1), Utc::now())
        .to_query()
        .fetch_optional(&pool)
        .await
        .expect("the repeated claim statement runs");
    assert!(reclaimed.is_none(), "a repeated claim takes nothing");
    assert_eq!(
        recorded_instant(&pool, original.write_id).await,
        original.recorded_at,
        "a repeated claim leaves the original instant in place"
    );

    // Storage still holds exactly one instance.
    assert_eq!(stored_history(&pool, &entity_id).await.len(), 1, "the replay duplicated nothing");
}

/// A record is released when the effects it protected go, and only those.
#[tokio::test]
#[serial]
async fn temporal_semantics_write_idempotency_records_release_only_with_their_own_effects() {
    let _serialized_environment = clean_database().await;
    let http = client();
    let pool = database().await;
    let released_id = unique_id("urn:ngsi-ld:Vehicle:idempotency_released");
    let retained_id = unique_id("urn:ngsi-ld:Vehicle:idempotency_retained");

    for entity_id in [&released_id, &retained_id] {
        let created = create_entity(
            &http,
            &json!({ "id": entity_id, "type": "Vehicle", "speed": { "type": "Property", "value": 1 } }),
        )
        .await;
        assert_eq!(created, StatusCode::CREATED);
    }

    let released_writes = stored_history(&pool, &released_id)
        .await
        .into_iter()
        .map(|row| row.write_id)
        .collect::<BTreeSet<Uuid>>()
        .into_iter()
        .collect::<Vec<Uuid>>();
    let retained_writes = stored_history(&pool, &retained_id)
        .await
        .into_iter()
        .map(|row| row.write_id)
        .collect::<BTreeSet<Uuid>>()
        .into_iter()
        .collect::<Vec<Uuid>>();
    assert!(!released_writes.is_empty() && !retained_writes.is_empty());
    assert_eq!(
        write_records_present(&pool, &released_writes).await,
        i64::try_from(released_writes.len()).expect("a small fixture is representable"),
        "a write that still protects history keeps its record"
    );

    // Clause 5.6.16 removes the history the record protected, so the record goes with it.
    assert_eq!(delete_temporal_entity(&http, &released_id).await, StatusCode::NO_CONTENT);
    assert_eq!(
        write_records_present(&pool, &released_writes).await,
        0,
        "records are released once nothing they protect remains"
    );
    assert_eq!(
        write_records_present(&pool, &retained_writes).await,
        i64::try_from(retained_writes.len()).expect("a small fixture is representable"),
        "a distinct write identity is untouched by another write's release"
    );
    assert_eq!(
        orphaned_temporal_rows(&pool).await,
        (0, 0, 0),
        "the deletion sequence left no temporal row whose owner is gone"
    );
}

/// The insert of a history row at a recording instant and write identity that
/// already hold one, copied from `original` onto its stream.
fn replay_history_row(original: &StoredHistoryRow) -> InsertStatement {
    Query::insert()
        .into_table(AttributeHistory::Table)
        .columns([
            AttributeHistory::RecordedAt,
            AttributeHistory::WriteId,
            AttributeHistory::StreamKey,
            AttributeHistory::WriteOrdinal,
            AttributeHistory::AttributeKind,
            AttributeHistory::ValueKind,
            AttributeHistory::GeneratedInstanceId,
            AttributeHistory::ValueNumber,
        ])
        .select_from(
            Query::select()
                .expr(Expr::val(original.recorded_at))
                .expr(Expr::val(original.write_id))
                .column(TemporalAttributeStreams::StreamKey)
                .expr(Expr::val(original.write_ordinal))
                .expr(Expr::val(original.attribute_kind))
                .expr(Expr::val(original.value_kind))
                .expr(Expr::val(Uuid::now_v7()))
                .expr(Expr::val(55))
                .from(TemporalAttributeStreams::Table)
                .and_where(Expr::col(TemporalAttributeStreams::StreamKey).eq(original.stream_key))
                .to_owned(),
        )
        .expect("the projection matches the column list")
        .to_owned()
}

/// The claim of a write record that takes nothing when the write already has
/// one.
fn claim_write_record(write_id: Uuid, recorded_at: DateTime<Utc>, accepted_at: DateTime<Utc>) -> InsertStatement {
    Query::insert()
        .into_table(WriteRecords::Table)
        .columns([WriteRecords::WriteId, WriteRecords::RecordedAt, WriteRecords::AcceptedAt])
        .values([Expr::val(write_id), Expr::val(recorded_at), Expr::val(accepted_at)])
        .expect("one value per column")
        .on_conflict(OnConflict::new().do_nothing().to_owned())
        .returning_col(WriteRecords::WriteId)
        .to_owned()
}
