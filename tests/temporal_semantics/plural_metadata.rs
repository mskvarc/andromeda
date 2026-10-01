//! Plural temporal metadata.
//!
//! A plural temporal member carries an envelope and a list of observations. The
//! envelope's metadata is shared by every member of the list, a nested member
//! overrides what it restates, and each member becomes its own typed
//! observation so that reconstruction is lossless.

use crate::{attribute_iri, client, create_temporal_entity, database, get_temporal_entity, rendered_instances, stored_history::stored_attribute_history};
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::{assert_eq, assert_ne};
use reqwest::StatusCode;
use serde_json::{Value, json};

/// The `datasetId` the envelope declares for every plural member.
const SHARED_DATASET: &str = "urn:ngsi-ld:Dataset:shared";

/// Every plural member of one attribute becomes its own typed observation,
/// inheriting envelope metadata unless the member overrides it.
#[tokio::test]
async fn temporal_semantics_plural_members_inherit_envelope_metadata_and_honour_overrides() {
    let scope = TestEnvironment::from_suite("temporal_semantics").await.expect("isolated test environment");
    let http = client();
    let pool = database().await;
    let entity_id = scope.id("Vehicle:plural_metadata");

    let (status, _) = create_temporal_entity(
        &http,
        &json!({
            "id": entity_id,
            "type": "Vehicle",
            "speed": {
                "type": "Property",
                "unitCode": "KMH",
                "datasetId": SHARED_DATASET,
                "values": [
                    { "value": 55, "observedAt": "2021-01-01T00:00:00Z" },
                    { "value": 61, "observedAt": "2021-01-02T00:00:00Z", "unitCode": "MTS" }
                ]
            }
        }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let rows = stored_attribute_history(&pool, &entity_id, &attribute_iri("speed")).await;
    assert_eq!(rows.len(), 2, "each plural member becomes its own observation");
    assert!(
        rows.iter().all(|row| row.dataset_id.as_deref() == Some(SHARED_DATASET)),
        "envelope metadata is inherited by every member"
    );
    assert_eq!(rows[0].value_number.as_deref(), Some("55"));
    assert_eq!(rows[1].value_number.as_deref(), Some("61"));
    assert_ne!(
        rows[0].unit_code_id, rows[1].unit_code_id,
        "the member that restates the unit overrides the envelope's"
    );

    // Reconstruction is lossless: both members return with the unit each of
    // them actually carries.
    let (status, body) = get_temporal_entity(&http, &entity_id, "").await;
    assert_eq!(status, StatusCode::OK);
    let instances = rendered_instances(&body, "speed");
    assert_eq!(instances.len(), 2);
    let units = instances
        .iter()
        .filter_map(|instance| instance.get("unitCode").and_then(Value::as_str))
        .collect::<Vec<&str>>();
    assert_eq!(units, vec!["KMH", "MTS"], "the inherited and the overriding unit both survive");
    assert!(
        instances.iter().all(|instance| instance["datasetId"] == json!(SHARED_DATASET)),
        "the inherited dataset identity survives"
    );
    scope.finish().await.expect("cleanup failed");
}

/// Every plural member kind becomes the correct typed observation.
#[tokio::test]
async fn temporal_semantics_every_plural_member_kind_becomes_its_typed_observation() {
    let scope = TestEnvironment::from_suite("temporal_semantics").await.expect("isolated test environment");
    let http = client();
    let pool = database().await;
    let entity_id = scope.id("Vehicle:plural_kinds");

    let (status, _) = create_temporal_entity(
        &http,
        &json!({
            "id": entity_id,
            "type": "Vehicle",
            "owner": {
                "type": "Relationship",
                "objects": [
                    { "object": "urn:ngsi-ld:Person:1", "observedAt": "2021-01-01T00:00:00Z" },
                    { "object": "urn:ngsi-ld:Person:2", "observedAt": "2021-01-02T00:00:00Z" }
                ]
            },
            "label": {
                "type": "LanguageProperty",
                "languageMaps": [
                    { "languageMap": { "en": "first" }, "observedAt": "2021-01-01T00:00:00Z" },
                    { "languageMap": { "en": "second" }, "observedAt": "2021-01-02T00:00:00Z" }
                ]
            },
            "doc": {
                "type": "JsonProperty",
                "jsons": [
                    { "json": { "a": 1 }, "observedAt": "2021-01-01T00:00:00Z" },
                    { "json": { "a": 2 }, "observedAt": "2021-01-02T00:00:00Z" }
                ]
            },
            "tags": {
                "type": "VocabProperty",
                "vocabs": [
                    { "vocab": "alpha", "observedAt": "2021-01-01T00:00:00Z" },
                    { "vocab": "beta", "observedAt": "2021-01-02T00:00:00Z" }
                ]
            }
        }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    // Stored family codes: 2 Relationship, 5 LanguageProperty, 6 JsonProperty,
    // 4 VocabProperty, as migration 0021 fixes them.
    for (term, attribute_kind) in [("owner", 2_i16), ("label", 5), ("doc", 6), ("tags", 4)] {
        let rows = stored_attribute_history(&pool, &entity_id, &attribute_iri(term)).await;
        assert_eq!(rows.len(), 2, "{term} recorded both plural members");
        assert!(
            rows.iter().all(|row| row.attribute_kind == attribute_kind),
            "{term} members are stored under their own attribute family"
        );
        assert!(
            rows.iter().all(|row| row.populated_value_columns == 1),
            "{term} members each populate exactly one typed value column"
        );
    }

    let (status, body) = get_temporal_entity(&http, &entity_id, "").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(rendered_instances(&body, "owner").len(), 2);
    assert_eq!(rendered_instances(&body, "owner")[1]["object"], json!("urn:ngsi-ld:Person:2"));
    assert_eq!(rendered_instances(&body, "label")[1]["languageMap"], json!({ "en": "second" }));
    assert_eq!(rendered_instances(&body, "doc")[1]["json"], json!({ "a": 2 }));
    assert_eq!(rendered_instances(&body, "tags")[1]["vocab"], json!("beta"));
    scope.finish().await.expect("cleanup failed");
}
