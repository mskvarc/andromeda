//! Compact generated instance identity and mixed UUID versions.
//!
//! Clause 4.5.7 hands a client-supplied `instanceId` back unchanged, so a
//! broker-minted identifier keeps only its UUID and is re-rendered through one
//! permanently stable template, while every other identifier is stored as exact
//! text. Exactly one of the two columns is populated, and both populations are
//! addressable for retrieval and deletion.

use crate::{
    attribute_iri,
    clean_database,
    client,
    create_temporal_entity,
    database,
    delete_temporal_instance,
    get_temporal_entity,
    rendered_instances,
    seeded_temporal::{SeededHistoryRow, attribute_name_id, seed_history_row, seed_stream, seed_temporal_entity},
    stored_history::stored_attribute_history,
    unique_id,
};
use chrono::{TimeDelta, Utc};
use reqwest::StatusCode;
use serde_json::{Value, json};
use serial_test::serial;
use uuid::Uuid;

/// The permanently stable template a broker-minted attribute instance renders through.
const GENERATED_INSTANCE_PREFIX: &str = "urn:ngsi-ld:attribute:instance:";

/// A client-supplied URN that resembles nothing the template mints.
const CLIENT_URN_INSTANCE: &str = "urn:ngsi-ld:my-own:instance:42";

/// A client-supplied URL identifier.
const CLIENT_URL_INSTANCE: &str = "https://example.org/instances/7";

/// The spelling earlier releases minted, carrying a trailing write ordinal the
/// one-value template cannot reproduce.
const LEGACY_GENERATED_INSTANCE: &str = "urn:ngsi-ld:attribute:instance:01942f6c-7f00-7000-8000-000000000001:0";

/// The rendered `instanceId` of every instance of one attribute.
fn rendered_instance_ids(body: &Value, term: &str) -> Vec<String> {
    rendered_instances(body, term)
        .iter()
        .filter_map(|instance| instance.get("instanceId").and_then(Value::as_str))
        .map(str::to_owned)
        .collect()
}

/// Broker-minted identifiers occupy the UUID column and render through the
/// template; every other identifier stays exact text in the verbatim column.
#[tokio::test]
#[serial]
async fn temporal_semantics_generated_and_verbatim_instance_identities_stay_apart() {
    let _serialized_environment = clean_database().await;
    let http = client();
    let pool = database().await;
    let entity_id = unique_id("urn:ngsi-ld:Vehicle:instance_identity");

    let (status, _) = create_temporal_entity(
        &http,
        &json!({
            "id": entity_id,
            "type": "Vehicle",
            "speed": [
                { "type": "Property", "value": 1, "observedAt": "2021-01-01T00:00:00Z" },
                { "type": "Property", "value": 2, "observedAt": "2021-01-02T00:00:00Z", "instanceId": CLIENT_URN_INSTANCE },
                { "type": "Property", "value": 3, "observedAt": "2021-01-03T00:00:00Z", "instanceId": CLIENT_URL_INSTANCE },
                { "type": "Property", "value": 4, "observedAt": "2021-01-04T00:00:00Z", "instanceId": LEGACY_GENERATED_INSTANCE }
            ]
        }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let rows = stored_attribute_history(&pool, &entity_id, &attribute_iri("speed")).await;
    assert_eq!(rows.len(), 4, "one row per submitted instance");
    for row in &rows {
        assert_eq!(
            i32::from(row.generated_instance_uuid.is_some()) + i32::from(row.verbatim_instance_id.is_some()),
            1,
            "exactly one instance-identity column is populated"
        );
    }

    let minted = rows.iter().filter(|row| row.generated_instance_uuid.is_some()).collect::<Vec<_>>();
    assert_eq!(minted.len(), 1, "only the unnamed instance is broker-minted");
    let minted_uuid = minted[0].generated_instance_uuid.expect("a minted identifier is a UUID");
    assert_eq!(minted_uuid.get_version_num(), 7, "minted instance identifiers are version 7");

    let verbatim = rows.iter().filter_map(|row| row.verbatim_instance_id.clone()).collect::<Vec<String>>();
    assert_eq!(
        verbatim,
        vec![
            CLIENT_URN_INSTANCE.to_owned(),
            CLIENT_URL_INSTANCE.to_owned(),
            LEGACY_GENERATED_INSTANCE.to_owned()
        ],
        "every identifier the template cannot reproduce is kept exactly as received"
    );

    // Rendering spells the minted identifier out from its UUID and hands every
    // other one back byte for byte.
    let (status, body) = get_temporal_entity(&http, &entity_id, "").await;
    assert_eq!(status, StatusCode::OK);
    let rendered = rendered_instance_ids(&body, "speed");
    assert!(
        rendered.contains(&format!("{GENERATED_INSTANCE_PREFIX}{}", minted_uuid.hyphenated())),
        "the minted identifier renders through the fixed template"
    );
    for supplied in [CLIENT_URN_INSTANCE, CLIENT_URL_INSTANCE, LEGACY_GENERATED_INSTANCE] {
        assert!(rendered.iter().any(|value| value == supplied), "{supplied} must be handed back unchanged");
    }

    // Both partial indexes serve addressing: each population is deletable by
    // the identifier it renders under, with no coercion between them.
    let minted_identifier = format!("{GENERATED_INSTANCE_PREFIX}{}", minted_uuid.hyphenated());
    assert_eq!(
        delete_temporal_instance(&http, &entity_id, "speed", &minted_identifier).await,
        StatusCode::NO_CONTENT,
        "a minted identifier addresses its own instance"
    );
    assert_eq!(
        delete_temporal_instance(&http, &entity_id, "speed", CLIENT_URN_INSTANCE).await,
        StatusCode::NO_CONTENT,
        "a verbatim identifier addresses its own instance"
    );
    let remaining = stored_attribute_history(&pool, &entity_id, &attribute_iri("speed")).await;
    assert_eq!(remaining.len(), 2, "exactly the two addressed instances were removed");
    assert!(
        remaining.iter().all(|row| row.verbatim_instance_id.is_some()),
        "the two surviving instances are the remaining verbatim ones"
    );
}

/// A migrated version-4 stream is read, joined and rendered beside a new
/// version-7 one, with no version-dependent branch and no rewritten identifier.
#[tokio::test]
#[serial]
async fn temporal_semantics_mixed_uuid_versions_read_and_join_without_rewriting() {
    let _serialized_environment = clean_database().await;
    let http = client();
    let pool = database().await;
    let migrated_id = unique_id("urn:ngsi-ld:Vehicle:mixed_uuid_migrated");
    let current_id = unique_id("urn:ngsi-ld:Vehicle:mixed_uuid_current");

    // A new write mints version-7 identifiers throughout.
    let (status, _) = create_temporal_entity(
        &http,
        &json!({
            "id": current_id,
            "type": "Vehicle",
            "speed": [{ "type": "Property", "value": 7, "observedAt": "2021-01-01T00:00:00Z" }]
        }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    // A migrated tenant carries version-4 stream and instance identifiers.
    let migrated_stream = Uuid::new_v4();
    let migrated_instance = Uuid::new_v4();
    assert_eq!(migrated_stream.get_version_num(), 4);
    let entity_key = seed_temporal_entity(&pool, &migrated_id, "Vehicle").await;
    let name_id = attribute_name_id(&pool, &attribute_iri("speed")).await;
    seed_stream(&pool, migrated_stream, entity_key, name_id).await;
    seed_history_row(
        &pool,
        &SeededHistoryRow {
            stream_id: migrated_stream,
            recorded_at: Utc::now() - TimeDelta::minutes(1),
            write_id: Uuid::new_v4(),
            generated_instance_uuid: migrated_instance,
            observed_at: "2021-01-01T00:00:00Z".parse().expect("a valid fixture instant"),
            value_number: 4,
        },
    )
    .await;

    // The migrated stream joins its catalogue row and renders like any other.
    let migrated_rows = stored_attribute_history(&pool, &migrated_id, &attribute_iri("speed")).await;
    assert_eq!(migrated_rows.len(), 1, "the version-4 stream joins its catalogue row");
    assert_eq!(migrated_rows[0].stream_id, migrated_stream, "the stored identifier is not rewritten");
    assert_eq!(
        migrated_rows[0].generated_instance_uuid,
        Some(migrated_instance),
        "the migrated instance identifier is not rewritten"
    );

    let current_rows = stored_attribute_history(&pool, &current_id, &attribute_iri("speed")).await;
    assert_eq!(current_rows[0].stream_id.get_version_num(), 7, "a new stream is version 7");
    assert_ne!(
        current_rows[0].stream_id.get_version_num(),
        migrated_rows[0].stream_id.get_version_num(),
        "the two populations really are different UUID versions"
    );

    // Both render through the same template and both are retrievable.
    for (entity_id, expected_value, expected_uuid) in [
        (&migrated_id, 4, migrated_instance),
        (&current_id, 7, current_rows[0].generated_instance_uuid.expect("a minted identifier")),
    ] {
        let (status, body) = get_temporal_entity(&http, entity_id, "").await;
        assert_eq!(status, StatusCode::OK, "a {expected_value}-valued stream is retrievable");
        let instances = rendered_instances(&body, "speed");
        assert_eq!(instances.len(), 1);
        assert_eq!(instances[0]["value"], json!(expected_value));
        assert_eq!(
            instances[0]["instanceId"].as_str(),
            Some(format!("{GENERATED_INSTANCE_PREFIX}{}", expected_uuid.hyphenated()).as_str()),
            "both UUID versions render through one template"
        );
    }
}

/// Clause 4.5.7 makes a client-supplied `instanceId` unique to one attribute
/// instance of one attribute, not to one `datasetId` stream of it. A write
/// naming the same `instanceId` on two sibling dataset streams of the same
/// attribute at once must be refused, not accepted onto both streams.
#[tokio::test]
#[serial]
async fn temporal_semantics_a_shared_instance_id_across_sibling_dataset_streams_in_one_write_is_refused() {
    let _serialized_environment = clean_database().await;
    let http = client();
    let pool = database().await;
    let entity_id = unique_id("urn:ngsi-ld:Vehicle:duplicate_instance_id");
    let shared_instance_id = "urn:ngsi-ld:my-own:instance:shared";

    let (status, _) = create_temporal_entity(
        &http,
        &json!({
            "id": entity_id,
            "type": "Vehicle",
            "speed": [
                {
                    "type": "Property",
                    "value": 1,
                    "observedAt": "2021-01-01T00:00:00Z",
                    "instanceId": shared_instance_id,
                    "datasetId": "urn:ngsi-ld:Dataset:a"
                },
                {
                    "type": "Property",
                    "value": 2,
                    "observedAt": "2021-01-02T00:00:00Z",
                    "instanceId": shared_instance_id,
                    "datasetId": "urn:ngsi-ld:Dataset:b"
                }
            ]
        }),
    )
    .await;
    assert!(
        !status.is_success(),
        "a write claiming one instanceId across two sibling dataset streams must be refused, got {status}"
    );

    let rows = stored_attribute_history(&pool, &entity_id, &attribute_iri("speed")).await;
    let claims = rows
        .iter()
        .filter(|row| row.verbatim_instance_id.as_deref() == Some(shared_instance_id))
        .count();
    assert!(
        claims <= 1,
        "the shared instanceId must never be stored on both sibling streams at once, found {claims} rows"
    );
}
