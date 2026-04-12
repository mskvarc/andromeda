//! Entity type details tests (NGSI-LD 5.5.9).
//! Uses `?details=true` to retrieve full `EntityType` objects with attribute names.

use reqwest::StatusCode;
use serde_json::Value;
use serial_test::serial;

use crate::{COMPOUND_CONTEXT_URL, clean_database, client, create_entity, delete_entity, list_entity_types, load_expectation, load_fixture, unique_id};

fn sorted_strings(val: &Value) -> Vec<String> {
    let mut v: Vec<String> = val.as_array().unwrap().iter().map(|s| s.as_str().unwrap().to_string()).collect();
    v.sort();
    v
}

fn sort_entity_types(val: &Value) -> Vec<Value> {
    let mut arr: Vec<Value> = val.as_array().unwrap().clone();
    arr.sort_by(|a, b| a["typeName"].as_str().unwrap().cmp(b["typeName"].as_str().unwrap()));
    for item in &mut arr {
        if let Some(names) = item.get_mut("attributeNames") {
            let mut sorted = sorted_strings(names);
            sorted.sort();
            *names = Value::Array(sorted.into_iter().map(Value::String).collect());
        }
    }
    arr
}

// ---------------------------------------------------------------------------
// 5.5.9 -- Without Link header context
// ---------------------------------------------------------------------------

/// List entity type details without Link header -- attribute names returned as expanded IRIs (5.5.9).
#[tokio::test]
#[serial]
async fn list_entity_type_details_without_context_returns_expanded_uris() {
    clean_database().await;
    let c = client();
    let building_id = unique_id("urn:ngsi-ld:Building:type-details-no-ctx");
    let vehicle_id = unique_id("urn:ngsi-ld:Vehicle:type-details-no-ctx");

    let building = load_fixture("entities/building-simple-attributes.json", &building_id);
    let vehicle = load_fixture("entities/vehicle-simple-attributes.json", &vehicle_id);

    let (s, _) = create_entity(&c, &building, Some(COMPOUND_CONTEXT_URL)).await;
    assert_eq!(s, StatusCode::CREATED);
    let (s, _) = create_entity(&c, &vehicle, Some(COMPOUND_CONTEXT_URL)).await;
    assert_eq!(s, StatusCode::CREATED);

    let (status, body) = list_entity_types(&c, "details=true", None).await;
    assert_eq!(status, StatusCode::OK);

    let expected = load_expectation("types/expectations/entity-type-023-01-01.json");
    assert_eq!(sort_entity_types(&body), sort_entity_types(&expected));

    delete_entity(&c, &building_id).await;
    delete_entity(&c, &vehicle_id).await;
}

// ---------------------------------------------------------------------------
// 5.5.9 -- With Link header context
// ---------------------------------------------------------------------------

/// List entity type details with compound context Link header -- attribute names compacted (5.5.9).
#[tokio::test]
#[serial]
async fn list_entity_type_details_with_context_returns_compacted_names() {
    clean_database().await;
    let c = client();
    let building_id = unique_id("urn:ngsi-ld:Building:type-details-with-ctx");
    let vehicle_id = unique_id("urn:ngsi-ld:Vehicle:type-details-with-ctx");

    let building = load_fixture("entities/building-simple-attributes.json", &building_id);
    let vehicle = load_fixture("entities/vehicle-simple-attributes.json", &vehicle_id);

    let (s, _) = create_entity(&c, &building, Some(COMPOUND_CONTEXT_URL)).await;
    assert_eq!(s, StatusCode::CREATED);
    let (s, _) = create_entity(&c, &vehicle, Some(COMPOUND_CONTEXT_URL)).await;
    assert_eq!(s, StatusCode::CREATED);

    let (status, body) = list_entity_types(&c, "details=true", Some(COMPOUND_CONTEXT_URL)).await;
    assert_eq!(status, StatusCode::OK);

    let expected = load_expectation("types/expectations/entity-type-023-01-02.json");
    assert_eq!(sort_entity_types(&body), sort_entity_types(&expected));

    delete_entity(&c, &building_id).await;
    delete_entity(&c, &vehicle_id).await;
}
