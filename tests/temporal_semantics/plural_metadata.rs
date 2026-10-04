//! Temporal Evolutions written in the simplified temporal representation.
//!
//! Clause 5.2.20 lets the `EntityTemporal` of a write be "specified by using the
//! Simplified temporal representation of an Entity" of clause 4.5.9: each
//! attribute holds its family's plural member of `[value, instant]` pairs, and
//! one object per `datasetId`. Each pair becomes its own typed observation of
//! the dataset its object names, so the history reads back as written.

use crate::{attribute_iri, client, create_temporal_entity, database, get_temporal_entity, rendered_instances, stored_history::stored_attribute_history};
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::json;

/// The `datasetId` one object of pairs is written under.
const SHARED_DATASET: &str = "urn:ngsi-ld:Dataset:shared";

/// Clauses 4.5.9 and 4.5.5: every pair becomes its own observation, of the
/// dataset the object holding it names, and the history reads back with each
/// instance in its dataset.
#[tokio::test]
async fn temporal_semantics_pairs_become_observations_of_their_object_dataset() {
    let scope = TestEnvironment::from_suite("temporal_semantics").await.expect("isolated test environment");
    let http = client();
    let pool = database().await;
    let entity_id = scope.id("Vehicle:plural_metadata");

    let (status, body) = create_temporal_entity(
        &http,
        &json!({
            "id": entity_id,
            "type": "Vehicle",
            "speed": [
                {
                    "type": "Property",
                    "datasetId": SHARED_DATASET,
                    "values": [[55, "2021-01-01T00:00:00Z"], [61, "2021-01-02T00:00:00Z"]]
                },
                {"type": "Property", "values": [[70, "2021-01-03T00:00:00Z"]]}
            ]
        }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body:?}");

    let rows = stored_attribute_history(&pool, &entity_id, &attribute_iri("speed")).await;
    assert_eq!(rows.len(), 3, "each pair becomes its own observation");
    let datasets: Vec<(Option<&str>, Option<&str>)> = rows.iter().map(|row| (row.value_number.as_deref(), row.dataset_id.as_deref())).collect();
    assert!(datasets.contains(&(Some("55"), Some(SHARED_DATASET))), "{datasets:?}");
    assert!(datasets.contains(&(Some("61"), Some(SHARED_DATASET))), "{datasets:?}");
    assert!(datasets.contains(&(Some("70"), None)), "{datasets:?}");

    let (status, body) = get_temporal_entity(&http, &entity_id, "").await;
    assert_eq!(status, StatusCode::OK);
    let instances = rendered_instances(&body, "speed");
    assert_eq!(instances.len(), 3, "{body}");
    let shared = instances.iter().filter(|instance| instance["datasetId"] == json!(SHARED_DATASET)).count();
    assert_eq!(shared, 2, "the dataset of the object survives on each of its pairs: {body}");
    scope.finish().await.expect("cleanup failed");
}

/// Clause 4.5.9 has an object of pairs "only contain" its plural member: an
/// instance member beside the pairs, which none of them could carry, is
/// refused with 400 and nothing is recorded.
#[tokio::test]
async fn temporal_semantics_a_member_beside_the_pairs_is_refused() {
    let scope = TestEnvironment::from_suite("temporal_semantics").await.expect("isolated test environment");
    let http = client();
    let pool = database().await;
    let entity_id = scope.id("Vehicle:plural_metadata_refused");

    let (status, body) = create_temporal_entity(
        &http,
        &json!({
            "id": entity_id,
            "type": "Vehicle",
            "speed": {"type": "Property", "unitCode": "KMH", "values": [[55, "2021-01-01T00:00:00Z"]]}
        }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body:?}");
    let rows = stored_attribute_history(&pool, &entity_id, &attribute_iri("speed")).await;
    assert!(rows.is_empty(), "nothing is recorded");
    scope.finish().await.expect("cleanup failed");
}

/// Clause 4.5.9 gives every family its own plural member, and wraps the value
/// of a `LanguageProperty`, `JsonProperty` and `VocabProperty` pair in a
/// one-member object; every pair becomes the typed observation of its family,
/// holding the value once.
#[tokio::test]
async fn temporal_semantics_every_family_of_pairs_becomes_its_typed_observation() {
    let scope = TestEnvironment::from_suite("temporal_semantics").await.expect("isolated test environment");
    let http = client();
    let pool = database().await;
    let entity_id = scope.id("Vehicle:plural_kinds");

    let (status, body) = create_temporal_entity(
        &http,
        &json!({
            "id": entity_id,
            "type": "Vehicle",
            "owner": {
                "type": "Relationship",
                "objects": [["urn:ngsi-ld:Person:1", "2021-01-01T00:00:00Z"], ["urn:ngsi-ld:Person:2", "2021-01-02T00:00:00Z"]]
            },
            "label": {
                "type": "LanguageProperty",
                "languageMaps": [
                    [{ "languageMap": { "en": "first" } }, "2021-01-01T00:00:00Z"],
                    [{ "languageMap": { "en": "second" } }, "2021-01-02T00:00:00Z"]
                ]
            },
            "doc": {
                "type": "JsonProperty",
                "jsons": [[{ "json": { "a": 1 } }, "2021-01-01T00:00:00Z"], [{ "json": { "a": 2 } }, "2021-01-02T00:00:00Z"]]
            },
            "tags": {
                "type": "VocabProperty",
                "vocabs": [[{ "vocab": "alpha" }, "2021-01-01T00:00:00Z"], [{ "vocab": "beta" }, "2021-01-02T00:00:00Z"]]
            }
        }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body:?}");

    // Stored family codes: 2 Relationship, 5 LanguageProperty, 6 JsonProperty,
    // 4 VocabProperty, as migration 0021 fixes them.
    for (term, attribute_kind) in [("owner", 2_i16), ("label", 5), ("doc", 6), ("tags", 4)] {
        let rows = stored_attribute_history(&pool, &entity_id, &attribute_iri(term)).await;
        assert_eq!(rows.len(), 2, "{term} recorded both pairs");
        assert!(
            rows.iter().all(|row| row.attribute_kind == attribute_kind),
            "{term} pairs are stored under their own attribute family"
        );
        assert!(
            rows.iter().all(|row| row.populated_value_columns == 1),
            "{term} pairs each populate exactly one typed value column"
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
