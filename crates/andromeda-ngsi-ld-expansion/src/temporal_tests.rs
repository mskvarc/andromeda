//! Typed expansion of the Temporal Evolution of an entity against the
//! processor's (clauses 4.5.6, 4.5.7 and 5.7.4.4): the instances of every
//! attribute, each with its own members, the Scope history and the entity's own
//! system times travel into the context-independent document the histories of
//! every holder are gathered in.

use crate::expansion_fixture::{TERMS, engine, temporal};
use andromeda_ngsi_ld_model::entity_system_times::EntitySystemTimes;
use pretty_assertions::assert_eq;
use serde_json::json;

const SPEED: &str = "https://example.org/terms#speed";

/// The instances of every attribute are read, each as the instance it is, and
/// the typed walk and the processor read them alike.
#[tokio::test]
async fn a_temporal_entity_expands_the_instances_of_every_attribute() {
    let document = temporal(
        &engine(),
        json!({
            "@context": TERMS,
            "id": "urn:ngsi-ld:Vehicle:1",
            "type": "Vehicle",
            "speed": [
                {"type": "Property", "value": 10, "observedAt": "2024-01-01T00:00:00Z", "instanceId": "urn:ngsi-ld:instance:1"},
                {"type": "Property", "value": 20, "observedAt": "2024-01-02T00:00:00Z", "instanceId": "urn:ngsi-ld:instance:2"},
                {"type": "Property", "value": 30, "observedAt": "2024-01-03T00:00:00Z", "datasetId": "urn:ngsi-ld:Dataset:a"}
            ],
            "sensor": [{"type": "Relationship", "object": "urn:ngsi-ld:Sensor:1", "observedAt": "2024-01-01T00:00:00Z"}]
        }),
    )
    .await
    .expect("the fixture expands both ways")
    .agreed();
    assert_eq!(document.instance_count(), 4);
    let speeds: Vec<_> = document.instances().filter(|(name, _)| name.as_str() == SPEED).collect();
    assert_eq!(speeds.len(), 3);
    assert_eq!(speeds[0].1.instance_id().map(ToString::to_string), Some("urn:ngsi-ld:instance:1".to_owned()));
    assert_eq!(speeds[2].1.dataset_id().map(ToString::to_string), Some("urn:ngsi-ld:Dataset:a".to_owned()));
}

/// The entity's own system times travel beside the instances, and the Scope
/// history is the attribute the Scope IRI names.
#[tokio::test]
async fn the_system_times_and_the_scope_history_travel_with_the_instances() {
    let document = temporal(
        &engine(),
        json!({
            "id": "urn:ngsi-ld:Vehicle:1",
            "type": "Vehicle",
            "createdAt": "2024-01-01T00:00:00Z",
            "modifiedAt": "2024-01-02T00:00:00.500Z",
            "scope": [
                {"type": "Property", "value": "/Madrid", "observedAt": "2024-01-01T00:00:00Z"},
                {"type": "Property", "value": "/Madrid/North", "observedAt": "2024-01-02T00:00:00Z"}
            ]
        }),
    )
    .await
    .expect("the fixture expands both ways")
    .agreed();
    let times = document.system_times();
    assert!(
        times.created_at.is_some() && times.modified_at.is_some() && times.deleted_at.is_none(),
        "{times:?}"
    );
    assert_eq!(times.modified_at.map(|at| at.timestamp_subsec_millis()), Some(500));
    assert_eq!(
        document
            .instances()
            .filter(|(name, _)| name.as_str() == "https://uri.etsi.org/ngsi-ld/scope")
            .count(),
        2
    );
    assert_ne!(times, EntitySystemTimes::default());
}

/// A deletion is one more instance, written as its family's null encoding
/// beside the `deletedAt` it happened at.
#[tokio::test]
async fn a_deletion_is_one_more_instance() {
    let document = temporal(
        &engine(),
        json!({
            "id": "urn:ngsi-ld:Vehicle:1",
            "type": "Vehicle",
            "https://example.org/terms#speed": [
                {"type": "Property", "value": 10, "observedAt": "2024-01-01T00:00:00Z"},
                {"type": "Property", "value": "urn:ngsi-ld:null", "deletedAt": "2024-01-02T00:00:00Z"}
            ]
        }),
    )
    .await
    .expect("the fixture expands both ways")
    .agreed();
    assert_eq!(document.instances().count(), 2);
}
