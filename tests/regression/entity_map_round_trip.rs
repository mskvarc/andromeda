//! An `EntityMap` reads back as it was created, through its typed model.
//!
//! ETSI GS CIM 009 v1.9.1 clause 5.2.32 defines the `EntityMap` data type
//! (Table 5.2.39-2: `id`, `type`, `entityMap`, `linkedMaps`, `expiresAt`) and
//! clause 5.5.14 has the broker mint one from a query and answer it on creation
//! with `201 Created`. Retrieving it by identifier (clause 6.3.19) has to answer
//! the same members the creation answered, apart from the timestamps the broker
//! generates; an `EntityMap` is created only from a query, so the creation
//! answer is the document that is written.
//!
//! Needs the host broker: the case drives the HTTP API and is not run without
//! one.
//!
//! Naming convention: `regression_<finding>_<scenario>`.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use crate::round_trip_comparison::assert_round_trips;
use andromeda_test_client::header::jsonld_link_header;
use andromeda_test_fixtures::context::TEST_SUITE_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::{Method, StatusCode};
use serde_json::json;
use urlencoding::encode;

/// Clauses 5.2.32 and 6.3.19: an `EntityMap` retrieved by identifier is the
/// `EntityMap` its creation answered.
#[tokio::test]
async fn regression_entity_map_round_trips_through_its_typed_model() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let entity_type = scope.entity_type("Vehicle");
    let ids: Vec<String> = (1..=2).map(|index| scope.id(&format!("Vehicle:em-typed-round-trip-{index}"))).collect();
    for id in &ids {
        let body = json!({"id": id, "type": entity_type, "speed": {"type": "Property", "value": 10}, "@context": [TEST_SUITE_CONTEXT]});
        let created = client.post_entity(&body).send().await.expect("create entity request");
        assert_eq!(created.status, StatusCode::CREATED, "create entity: {:?}", created.body);
    }

    let minted = client
        .raw(
            Method::GET,
            &format!("/ngsi-ld/v1/entityMaps?{}", scope.scope_query(&format!("type={entity_type}"))),
        )
        .header("Accept", "application/ld+json")
        .header("Link", jsonld_link_header(TEST_SUITE_CONTEXT).expect("test-suite context Link must be valid"))
        .send()
        .await
        .expect("create EntityMap request");
    assert_eq!(minted.status, StatusCode::CREATED, "create EntityMap: {:?}", minted.body);
    let written = minted.json_value().expect("a created EntityMap is JSON");
    let map_id = written["id"].as_str().expect("the created EntityMap carries an id").to_owned();

    let retrieved = client
        .raw(Method::GET, &format!("/ngsi-ld/v1/entityMaps/{}", encode(&map_id)))
        .header("Accept", "application/ld+json")
        .header("Link", jsonld_link_header(TEST_SUITE_CONTEXT).expect("test-suite context Link must be valid"))
        .send()
        .await
        .expect("retrieve EntityMap request");
    assert_eq!(retrieved.status, StatusCode::OK, "retrieve EntityMap: {:?}", retrieved.body);
    let returned = retrieved.json_value().expect("a retrieved EntityMap is JSON");

    assert_eq!(
        returned["entityMap"].as_object().map(serde_json::Map::len),
        Some(ids.len()),
        "the map lists the entities it was minted over: {returned}"
    );
    assert_round_trips(written, returned, "EntityMap");

    client.delete_entity_map(&map_id).await.expect("delete EntityMap request");
    scope.finish().await.expect("environment cleanup");
}
