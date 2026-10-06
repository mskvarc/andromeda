//! A deleted `GeoProperty` read back through the Temporal API.
//!
//! ETSI GS CIM 009 v1.9.1 clause 4.5.0 encodes the deletion of a value with the
//! NGSI-LD Null, "the URI `urn:ngsi-ld:null` ... used as a Property value" in the
//! temporal representation, and clauses 4.5.7 and 4.5.8 render an instance that
//! has ended with the Null its family prescribes beside its `deletedAt`. A
//! `GeoProperty` is the one family whose value member is otherwise a geometry,
//! so its deletion is the shape a typed reading of a temporal evolution is most
//! likely to refuse. The retrieval has to answer it, in both temporal
//! representations, rather than fail the whole evolution as invalid.
//!
//! Naming convention: `regression_<finding>_<scenario>`.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use andromeda_test_client::entity_attribute::DatasetSelection;
use andromeda_test_fixtures::context::CORE_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::{Value, json};

/// The NGSI-LD Null of clause 4.5.0.
const NULL_URI: &str = "urn:ngsi-ld:null";

/// The query every temporal read of this module asks for: no time filter,
/// because clause 4.11 makes `observedAt` the default `timeproperty`, which no
/// instance of a Core-written entity carries.
const WINDOW: &str = "";

/// Clauses 4.5.0, 4.5.7 and 4.5.8: after the `location` of an entity is deleted,
/// its Temporal Evolution still answers 200, and the ended instance carries the
/// NGSI-LD Null in the normalized representation and in `temporalValues`.
#[tokio::test]
async fn regression_a_deleted_geo_property_in_a_temporal_evolution_reads_typed() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:deleted-geo");

    let created = client
        .post_entity(&json!({
            "id": id,
            "type": "Building",
            "location": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [13.4, 52.5]}},
            "@context": [CORE_CONTEXT]
        }))
        .with_ld_json()
        .send()
        .await
        .expect("create entity request");
    assert_eq!(created.status, StatusCode::CREATED, "create: {:?}", created.body);

    let deleted = client
        .delete_entity_attr(&id, "location", None, DatasetSelection::Default, Some(CORE_CONTEXT))
        .await
        .expect("delete attribute request");
    assert_eq!(deleted.status, StatusCode::NO_CONTENT, "delete attribute: {:?}", deleted.body);

    let normalized = client
        .get_temporal_entity(&id, WINDOW, Some(CORE_CONTEXT))
        .await
        .expect("retrieve temporal evolution request");
    assert_eq!(
        normalized.status,
        StatusCode::OK,
        "no InvalidEntity for a deleted GeoProperty: {:?}",
        normalized.body
    );
    let evolution = normalized.json_value().expect("a temporal evolution is JSON");
    let instances = evolution["location"]
        .as_array()
        .expect("the temporal evolution of an attribute is an array of instances");
    assert!(
        instances
            .iter()
            .any(|instance| instance["type"] == json!("GeoProperty") && instance["value"] == json!(NULL_URI)),
        "clause 4.5.7: the deletion is a GeoProperty instance whose value is the NGSI-LD Null: {evolution}"
    );
    assert!(
        instances
            .iter()
            .any(|instance| instance["value"]["type"] == json!("Point") && instance["value"]["coordinates"] == json!([13.4, 52.5])),
        "the observation before the deletion keeps its geometry: {evolution}"
    );

    let simplified = client
        .get_temporal_entity(&id, "format=temporalValues", Some(CORE_CONTEXT))
        .await
        .expect("retrieve simplified temporal evolution request");
    assert_eq!(simplified.status, StatusCode::OK, "temporalValues: {:?}", simplified.body);
    let simplified: Value = simplified.json_value().expect("a temporal answer is JSON");
    let values = simplified["location"]["values"]
        .as_array()
        .expect("clause 4.5.8: the simplified evolution of an attribute holds a values array");
    assert!(
        values.iter().any(|pair| pair[0] == json!(NULL_URI)),
        "clause 4.5.8: the ended instance is the NGSI-LD Null in the values array: {simplified}"
    );

    scope.finish().await.expect("environment cleanup");
}
