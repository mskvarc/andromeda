#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]
//! Entity type details tests (NGSI-LD 5.5.9).
//! Uses `?details=true` to retrieve full `EntityType` objects with attribute names.

use reqwest::StatusCode;
use serde_json::Value;

use crate::{
    COMPOUND_CONTEXT_URL,
    building_simple_attributes,
    client,
    create_entity,
    delete_entity,
    expected_entity_type_details_compacted,
    expected_entity_type_details_expanded,
    list_entity_types,
    unique_id,
    vehicle_simple_attributes,
};
use andromeda_test_harness::tenant_scoped_environment::TenantScopedEnvironment;

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
async fn list_entity_type_details_without_context_returns_expanded_uris() {
    let env = TenantScopedEnvironment::from_suite("discovery").await.expect("private tenant environment");
    let c = client();
    let building_id = unique_id("urn:ngsi-ld:Building:type-details-no-ctx");
    let vehicle_id = unique_id("urn:ngsi-ld:Vehicle:type-details-no-ctx");

    let building = building_simple_attributes(&building_id);
    let vehicle = vehicle_simple_attributes(&vehicle_id);

    let (s, _) = create_entity(&c, env.tenant(), &building, Some(COMPOUND_CONTEXT_URL)).await;
    pretty_assertions::assert_eq!(s, StatusCode::CREATED);
    let (s, _) = create_entity(&c, env.tenant(), &vehicle, Some(COMPOUND_CONTEXT_URL)).await;
    pretty_assertions::assert_eq!(s, StatusCode::CREATED);

    let (status, body) = list_entity_types(&c, env.tenant(), "details=true", None).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    let expected = expected_entity_type_details_expanded();
    pretty_assertions::assert_eq!(sort_entity_types(&body), sort_entity_types(&expected));

    delete_entity(&c, env.tenant(), &building_id).await;
    delete_entity(&c, env.tenant(), &vehicle_id).await;
    env.finish().await.expect("cleanup failed");
}

// ---------------------------------------------------------------------------
// 5.5.9 -- With Link header context
// ---------------------------------------------------------------------------

/// List entity type details with compound context Link header -- attribute names compacted (5.5.9).
#[tokio::test]
async fn list_entity_type_details_with_context_returns_compacted_names() {
    let env = TenantScopedEnvironment::from_suite("discovery").await.expect("private tenant environment");
    let c = client();
    let building_id = unique_id("urn:ngsi-ld:Building:type-details-with-ctx");
    let vehicle_id = unique_id("urn:ngsi-ld:Vehicle:type-details-with-ctx");

    let building = building_simple_attributes(&building_id);
    let vehicle = vehicle_simple_attributes(&vehicle_id);

    let (s, _) = create_entity(&c, env.tenant(), &building, Some(COMPOUND_CONTEXT_URL)).await;
    pretty_assertions::assert_eq!(s, StatusCode::CREATED);
    let (s, _) = create_entity(&c, env.tenant(), &vehicle, Some(COMPOUND_CONTEXT_URL)).await;
    pretty_assertions::assert_eq!(s, StatusCode::CREATED);

    let (status, body) = list_entity_types(&c, env.tenant(), "details=true", Some(COMPOUND_CONTEXT_URL)).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    let expected = expected_entity_type_details_compacted();
    pretty_assertions::assert_eq!(sort_entity_types(&body), sort_entity_types(&expected));

    delete_entity(&c, env.tenant(), &building_id).await;
    delete_entity(&c, env.tenant(), &vehicle_id).await;
    env.finish().await.expect("cleanup failed");
}
