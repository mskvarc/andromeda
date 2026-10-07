#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]
//! Entity type info retrieval tests (NGSI-LD 5.5.10, 6.3.5).

use reqwest::StatusCode;
use serde_json::Value;

use crate::{
    COMPOUND_CONTEXT_URL,
    RESOURCE_NOT_FOUND_TYPE,
    building_simple_attributes,
    client,
    create_entity,
    delete_entity,
    expected_entity_type_info,
    retrieve_entity_type,
    unique_id,
};
use andromeda_test_harness::tenant_scoped_environment::TenantScopedEnvironment;

fn sort_attribute_details(details: &mut [Value]) {
    details.sort_by(|a, b| a["attributeName"].as_str().unwrap().cmp(b["attributeName"].as_str().unwrap()));
}

/// Retrieve type info for a compacted name with no context -- 404 because the name cannot be resolved (5.5.10).
#[tokio::test]
async fn retrieve_entity_type_info_unknown_type_returns_404() {
    let env = TenantScopedEnvironment::from_suite("discovery").await.expect("private tenant environment");
    let c = client();
    let id = unique_id("urn:ngsi-ld:Building:type-info-404");
    let entity = building_simple_attributes(&id);

    let (s, _) = create_entity(&c, env.tenant(), &entity, Some(COMPOUND_CONTEXT_URL)).await;
    pretty_assertions::assert_eq!(s, StatusCode::CREATED);

    // "Building" is a compacted name; without a context it cannot be expanded to a known IRI
    let (status, body) = retrieve_entity_type(&c, env.tenant(), "Building", None).await;
    pretty_assertions::assert_eq!(status, StatusCode::NOT_FOUND);
    pretty_assertions::assert_eq!(body["type"], RESOURCE_NOT_FOUND_TYPE.as_str());
    assert!(body["title"].as_str().is_some_and(|t| !t.is_empty()), "title should be a non-empty string");

    delete_entity(&c, env.tenant(), &id).await;
    env.finish().await.expect("cleanup failed");
}

/// Retrieve entity type info by compacted name with context Link header -- returns `EntityTypeInfo` (5.5.10, 6.3.5).
#[tokio::test]
async fn retrieve_entity_type_info_with_compacted_name_returns_type_info() {
    let env = TenantScopedEnvironment::from_suite("discovery").await.expect("private tenant environment");
    let c = client();
    let id1 = unique_id("urn:ngsi-ld:Building:type-info-compact-a");
    let id2 = unique_id("urn:ngsi-ld:Building:type-info-compact-b");

    let e1 = building_simple_attributes(&id1);
    let e2 = building_simple_attributes(&id2);

    let (s, _) = create_entity(&c, env.tenant(), &e1, Some(COMPOUND_CONTEXT_URL)).await;
    pretty_assertions::assert_eq!(s, StatusCode::CREATED);
    let (s, _) = create_entity(&c, env.tenant(), &e2, Some(COMPOUND_CONTEXT_URL)).await;
    pretty_assertions::assert_eq!(s, StatusCode::CREATED);

    let (status, body) = retrieve_entity_type(&c, env.tenant(), "Building", Some(COMPOUND_CONTEXT_URL)).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    let expected = expected_entity_type_info();
    pretty_assertions::assert_eq!(body["type"], expected["type"]);
    pretty_assertions::assert_eq!(body["typeName"], expected["typeName"]);
    pretty_assertions::assert_eq!(body["entityCount"], expected["entityCount"]);

    let mut body_details: Vec<Value> = body["attributeDetails"].as_array().unwrap().clone();
    sort_attribute_details(&mut body_details);
    let mut expected_details: Vec<Value> = expected["attributeDetails"].as_array().unwrap().clone();
    sort_attribute_details(&mut expected_details);
    pretty_assertions::assert_eq!(body_details, expected_details);

    delete_entity(&c, env.tenant(), &id1).await;
    delete_entity(&c, env.tenant(), &id2).await;
    env.finish().await.expect("cleanup failed");
}

/// Retrieve entity type info by URL-encoded expanded IRI -- returns same `EntityTypeInfo` as compacted (5.5.10, 6.3.5).
#[tokio::test]
async fn retrieve_entity_type_info_with_expanded_name_returns_type_info() {
    let env = TenantScopedEnvironment::from_suite("discovery").await.expect("private tenant environment");
    let c = client();
    let id1 = unique_id("urn:ngsi-ld:Building:type-info-expanded-a");
    let id2 = unique_id("urn:ngsi-ld:Building:type-info-expanded-b");

    let e1 = building_simple_attributes(&id1);
    let e2 = building_simple_attributes(&id2);

    let (s, _) = create_entity(&c, env.tenant(), &e1, Some(COMPOUND_CONTEXT_URL)).await;
    pretty_assertions::assert_eq!(s, StatusCode::CREATED);
    let (s, _) = create_entity(&c, env.tenant(), &e2, Some(COMPOUND_CONTEXT_URL)).await;
    pretty_assertions::assert_eq!(s, StatusCode::CREATED);

    // "https://ngsi-ld-test-suite/context#Building" percent-encoded
    let expanded_type = "https%3A%2F%2Fngsi-ld-test-suite%2Fcontext%23Building";
    let (status, body) = retrieve_entity_type(&c, env.tenant(), expanded_type, Some(COMPOUND_CONTEXT_URL)).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    let expected = expected_entity_type_info();
    pretty_assertions::assert_eq!(body["type"], expected["type"]);
    pretty_assertions::assert_eq!(body["typeName"], expected["typeName"]);
    pretty_assertions::assert_eq!(body["entityCount"], expected["entityCount"]);

    let mut body_details: Vec<Value> = body["attributeDetails"].as_array().unwrap().clone();
    sort_attribute_details(&mut body_details);
    let mut expected_details: Vec<Value> = expected["attributeDetails"].as_array().unwrap().clone();
    sort_attribute_details(&mut expected_details);
    pretty_assertions::assert_eq!(body_details, expected_details);

    delete_entity(&c, env.tenant(), &id1).await;
    delete_entity(&c, env.tenant(), &id2).await;
    env.finish().await.expect("cleanup failed");
}
