//! Operator retention.
//!
//! Retention is a storage decision an operator takes about their own tenant and
//! is not an NGSI-LD operation: a dropped chunk records no tombstone and raises
//! no notification, unlike Temporal Evolution deletion (clause 5.6.16). The
//! sweep drops aged chunks, garbage-collects the catalogue rows whose history
//! left with them, and releases the idempotency records that are fully behind
//! the horizon and protect nothing.

use crate::{
    attribute_iri,
    clean_database,
    client,
    create_entity,
    database,
    get_temporal_entity,
    rendered_instances,
    seeded_temporal::{SeededHistoryRow, attribute_name_id, seed_history_row, seed_stream, seed_temporal_entity},
    stored_history::{stored_history, temporal_catalog_rows},
    unique_id,
};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName};
use andromeda_postgresql_name_dictionary::name_cache::NameCache;
use andromeda_postgresql_temporal_store::{
    bulk_write_mode::BulkWriteMode,
    repo::TemporalRepository,
    retention_sweep::{TemporalRetention, TemporalRetentionHorizon, TemporalRetentionOutcome},
};
use chrono::{TimeDelta, Utc};
use reqwest::StatusCode;
use serde_json::json;
use serial_test::serial;
use sqlx::{PgPool, Row};
use std::sync::Arc;
use uuid::Uuid;

/// How far back the fixture operator keeps temporal storage.
const RETENTION_DAYS: i64 = 90;

/// How far behind the horizon the aged fixture is placed.
const AGED_DAYS: i64 = 400;

/// Count the idempotency records carrying one write identity.
async fn write_records_present(pool: &PgPool, write_id: Uuid) -> i64 {
    sqlx::query("SELECT count(*)::bigint AS records FROM write_records WHERE write_id = $1")
        .bind(write_id)
        .fetch_one(pool)
        .await
        .expect("idempotency record count")
        .get("records")
}

/// Count every tombstone row in the tenant.
async fn tombstone_rows(pool: &PgPool) -> i64 {
    sqlx::query("SELECT count(*)::bigint AS rows FROM attribute_history WHERE value_kind = 0")
        .fetch_one(pool)
        .await
        .expect("tombstone row count")
        .get("rows")
}

/// A configured horizon ages storage out, collects what it emptied, and
/// releases the idempotency records behind it, while leaving current storage
/// and its records alone.
#[tokio::test]
#[serial]
async fn temporal_semantics_operator_retention_ages_storage_out_without_deletion_semantics() {
    let _serialized_environment = clean_database().await;
    let http = client();
    let pool = database().await;
    let aged_id = unique_id("urn:ngsi-ld:Sensor:retention_aged");
    let retained_id = unique_id("urn:ngsi-ld:Sensor:retention_retained");

    // Storage the horizon keeps: a write made now.
    let created = create_entity(
        &http,
        &json!({
            "id": retained_id,
            "type": "Sensor",
            "reading": { "type": "Property", "value": 55, "observedAt": "2021-01-01T00:00:00Z" }
        }),
    )
    .await;
    assert_eq!(created, StatusCode::CREATED);
    let retained_write = stored_history(&pool, &retained_id)
        .await
        .first()
        .map(|row| row.write_id)
        .expect("the retained write recorded a row");

    // Storage the horizon ages out: a write recorded well behind it.
    let aged_write = Uuid::now_v7();
    let entity_key = seed_temporal_entity(&pool, &aged_id, "Sensor").await;
    let name_id = attribute_name_id(&pool, &attribute_iri("reading")).await;
    let aged_stream = seed_stream(&pool, entity_key, name_id).await;
    seed_history_row(
        &pool,
        &SeededHistoryRow {
            stream_key: aged_stream,
            recorded_at: Utc::now() - TimeDelta::days(AGED_DAYS),
            write_id: aged_write,
            generated_instance_uuid: Uuid::now_v7(),
            observed_at: "2020-01-01T00:00:00Z".parse().expect("a valid fixture instant"),
            value_number: 1,
        },
    )
    .await;
    assert_eq!(stored_history(&pool, &aged_id).await.len(), 1, "the aged fixture is in storage");
    let tombstones_before = tombstone_rows(&pool).await;

    let repository = TemporalRepository::new(
        &pool,
        BulkWriteMode::Unnest,
        Arc::new(NameCache::<AttributeName>::new(&pool)),
        Arc::new(NameCache::<EntityTypeName>::new(&pool)),
    );
    let horizon = TemporalRetentionHorizon::new(TimeDelta::days(RETENTION_DAYS)).expect("a positive retention horizon");
    let outcome = repository
        .sweep_retention(TemporalRetention::Enabled { horizon }, Utc::now())
        .await
        .expect("the retention sweep runs");
    let sweep = match outcome {
        TemporalRetentionOutcome::Disabled => panic!("retention was configured for this sweep"),
        TemporalRetentionOutcome::Swept(sweep) => sweep,
    };

    assert!(sweep.attribute_history_chunks >= 1, "at least the aged chunk was dropped");
    assert!(sweep.streams >= 1, "the stream left without history was collected");
    assert!(sweep.temporal_entities >= 1, "the temporal Entity left without streams was collected");
    assert!(sweep.released_write_records >= 1, "the record behind the horizon was released");

    // The aged tenant state is gone, catalogue included.
    assert!(stored_history(&pool, &aged_id).await.is_empty(), "aged history left storage");
    assert_eq!(temporal_catalog_rows(&pool, &aged_id).await, (0, 0), "its catalogue rows were collected");
    assert_eq!(write_records_present(&pool, aged_write).await, 0, "its idempotency record was released");

    // Retention is not deletion: nothing was recorded about what left.
    assert_eq!(
        tombstone_rows(&pool).await,
        tombstones_before,
        "a dropped chunk records no specification deletion"
    );

    // Current storage and its idempotency record are untouched.
    assert_eq!(stored_history(&pool, &retained_id).await.len(), 1, "recent history is kept");
    assert_eq!(temporal_catalog_rows(&pool, &retained_id).await, (1, 1), "its catalogue rows are kept");
    assert_eq!(
        write_records_present(&pool, retained_write).await,
        1,
        "a record that still protects an effect is not released"
    );
    let (status, body) = get_temporal_entity(&http, &retained_id, "").await;
    assert_eq!(status, StatusCode::OK);
    assert!(!rendered_instances(&body, "reading").is_empty(), "retained history is still queryable");
}

/// A broker that was given no horizon ages nothing out.
#[tokio::test]
#[serial]
async fn temporal_semantics_operator_retention_is_off_until_a_horizon_is_configured() {
    let _serialized_environment = clean_database().await;
    let http = client();
    let pool = database().await;
    let entity_id = unique_id("urn:ngsi-ld:Sensor:retention_disabled");

    let created = create_entity(
        &http,
        &json!({ "id": entity_id, "type": "Sensor", "reading": { "type": "Property", "value": 1 } }),
    )
    .await;
    assert_eq!(created, StatusCode::CREATED);

    let repository = TemporalRepository::new(
        &pool,
        BulkWriteMode::Unnest,
        Arc::new(NameCache::<AttributeName>::new(&pool)),
        Arc::new(NameCache::<EntityTypeName>::new(&pool)),
    );
    let outcome = repository
        .sweep_retention(TemporalRetention::Disabled, Utc::now())
        .await
        .expect("a disabled sweep still reports");
    assert_eq!(outcome, TemporalRetentionOutcome::Disabled, "an unconfigured broker sweeps nothing");
    assert_eq!(stored_history(&pool, &entity_id).await.len(), 1, "nothing left storage");
}
