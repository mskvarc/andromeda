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
    stored_history::{orphaned_temporal_rows, stored_history},
    unique_id,
};
use chrono::{DateTime, TimeDelta, Utc};
use reqwest::StatusCode;
use serde_json::json;
use serial_test::serial;
use sqlx::{PgPool, Row};
use std::collections::BTreeSet;
use uuid::Uuid;

/// Count the idempotency records among a named set of write identities.
async fn write_records_present(pool: &PgPool, write_ids: &[Uuid]) -> i64 {
    sqlx::query("SELECT count(*)::bigint AS records FROM write_records WHERE write_id = ANY($1)")
        .bind(write_ids)
        .fetch_one(pool)
        .await
        .expect("idempotency record count")
        .get("records")
}

/// Read the instant one idempotency record persists.
async fn recorded_instant(pool: &PgPool, write_id: Uuid) -> DateTime<Utc> {
    sqlx::query("SELECT recorded_at FROM write_records WHERE write_id = $1")
        .bind(write_id)
        .fetch_one(pool)
        .await
        .expect("the idempotency record of a committed write")
        .get("recorded_at")
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
    let replayed = sqlx::query(REPLAY_HISTORY_ROW_SQL)
        .bind(original.recorded_at)
        .bind(original.write_id)
        .bind(original.stream_key)
        .bind(original.write_ordinal)
        .bind(original.attribute_kind)
        .bind(original.value_kind)
        .bind(Uuid::now_v7())
        .execute(&pool)
        .await;
    let violation = replayed.expect_err("a replayed consequence must not be stored twice");
    assert_eq!(
        violation.as_database_error().and_then(sqlx::error::DatabaseError::code).as_deref(),
        Some("23505"),
        "the collision is a uniqueness violation on (recorded_at, write_id, write_ordinal)"
    );

    // Claiming the same write again is recognised rather than recorded twice,
    // and it does not move the persisted instant a replay has to reuse.
    let reclaimed = sqlx::query(CLAIM_WRITE_RECORD_SQL)
        .bind(original.write_id)
        .bind(original.recorded_at + TimeDelta::hours(1))
        .bind(Utc::now())
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

const REPLAY_HISTORY_ROW_SQL: &str = r"
INSERT INTO attribute_history
    (recorded_at, context_space_key, write_id, stream_key, write_ordinal, attribute_kind, value_kind,
     generated_instance_uuid, value_number)
SELECT $1, context_space_key, $2, stream_key, $4, $5, $6, $7, 55
FROM temporal_attribute_streams WHERE stream_key = $3
";

const CLAIM_WRITE_RECORD_SQL: &str = r"
INSERT INTO write_records (write_id, recorded_at, accepted_at)
VALUES ($1, $2, $3)
ON CONFLICT DO NOTHING
RETURNING write_id
";
