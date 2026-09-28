//! A `datasetId` filter that matches no instance removes Attributes from a
//! temporal retrieval, never the Entity.
//!
//! ETSI GS CIM 009 v1.9.1 clause 5.7.3.4: step S1 removes an Entity only when
//! "the time series, for all the concerned Attributes of an Entity, does not
//! include data corresponding to the temporal query interval"; the `datasetId`
//! step runs on S1 afterwards and, when "there is no Attribute instance whose
//! datasetId matches the value of the parameter", removes "the complete
//! Attribute from S1". The broker judged S1 after the dataset filter, so a
//! dataset no instance carries answered 404 for an Entity whose history the
//! window selected.
//!
//! Naming convention: `regression_<finding>_<scenario>`.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::json;

/// The core context every payload here is written against.
const CORE_CONTEXT: &str = "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld";

/// A window every observation of this module falls inside.
const BEFORE_2030: &str = "timerel=before&timeAt=2030-01-01T00:00:00Z";

#[tokio::test]
async fn regression_temporal_dataset_filter_a_dataset_no_instance_carries_keeps_the_entity() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let entity_type = scope.entity_type("TdsFilter");
    let id = scope.id(&format!("{entity_type}:1"));
    let named = scope.id("dataset:named");
    let created = scope
        .client()
        .create_temporal_entity(&json!({
            "id": id,
            "type": entity_type,
            "speed": [{"type": "Property", "value": 10, "observedAt": "2020-08-01T12:00:00Z"}],
            "fuel": [{"type": "Property", "value": 40, "observedAt": "2020-08-01T12:00:00Z", "datasetId": named}],
            "@context": CORE_CONTEXT
        }))
        .await
        .expect("create temporal entity request");
    assert_eq!(created.status, StatusCode::CREATED, "{:?}", created.body);

    let absent = urlencoding::encode(&scope.id("dataset:absent")).into_owned();
    let retrieved = scope
        .client()
        .get_temporal_entity(&id, &format!("{BEFORE_2030}&datasetId={absent}"), Some(CORE_CONTEXT))
        .await
        .expect("retrieve request");
    assert_eq!(retrieved.status, StatusCode::OK, "{:?}", retrieved.body);
    let body = retrieved.json_value().expect("a temporal entity is JSON");
    assert_eq!(body["id"], json!(id));
    assert!(body.get("speed").is_none() && body.get("fuel").is_none(), "every Attribute is removed: {body}");

    // The named dataset keeps its own Attribute alone.
    let encoded = urlencoding::encode(&named).into_owned();
    let narrowed = scope
        .client()
        .get_temporal_entity(&id, &format!("{BEFORE_2030}&datasetId={encoded}"), Some(CORE_CONTEXT))
        .await
        .expect("retrieve request");
    assert_eq!(narrowed.status, StatusCode::OK, "{:?}", narrowed.body);
    let body = narrowed.json_value().expect("a temporal entity is JSON");
    assert!(body.get("speed").is_none(), "{body}");
    assert!(body.get("fuel").is_some(), "{body}");

    // A window with no data still removes the Entity (step S1).
    let empty = scope
        .client()
        .get_temporal_entity(&id, "timerel=before&timeAt=2000-01-01T00:00:00Z&datasetId=%40none", Some(CORE_CONTEXT))
        .await
        .expect("retrieve request");
    assert_eq!(empty.status, StatusCode::NOT_FOUND, "{:?}", empty.body);

    scope.finish().await.expect("cleanup failed");
}
