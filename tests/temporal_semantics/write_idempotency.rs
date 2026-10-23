//! Write idempotency.
//!
//! Every recorded instance carries the instant its write was recorded at, the
//! write's identity and its position in the write. A replay of a queued intent
//! carries the same three, so it collides with the history rows it would
//! otherwise duplicate on the hypertable's uniqueness key.

use crate::{
    clean_database,
    client,
    create_entity,
    database,
    stored_history::{StoredHistoryRow, stored_history},
    unique_id,
    update_entity_attributes,
};
use andromeda_postgresql_schema::temporal::{AttributeHistory, TemporalAttributeStreams};
use andromeda_postgresql_seaquery::statement::Statement;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use sea_query::{Expr, ExprTrait, InsertStatement, Query};
use serde_json::json;
use serial_test::serial;
use sqlx::error::DatabaseError;
use uuid::Uuid;

/// A replayed instance carries its write's instant and identity, so the
/// hypertable's uniqueness key rejects the repeat rather than storing history
/// twice.
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
    assert_eq!(
        update_entity_attributes(&http, &entity_id, &json!({ "speed": { "type": "Property", "value": 56 } })).await,
        StatusCode::NO_CONTENT
    );

    // The replaced instance moved into history; the current one stays on its row.
    let rows = stored_history(&pool, &entity_id).await;
    assert_eq!(rows.len(), 2, "the fixture records two instances");
    let original = rows[0].clone();

    let replayed = replay_history_row(&original).to_query().execute(&pool).await;
    let violation = replayed.expect_err("a replayed consequence must not be stored twice");
    assert_eq!(
        violation.as_database_error().and_then(DatabaseError::code).as_deref(),
        Some("23505"),
        "the collision is a uniqueness violation on (recorded_at, write_id, write_ordinal)"
    );

    assert_eq!(stored_history(&pool, &entity_id).await.len(), 2, "the replay duplicated nothing");
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
