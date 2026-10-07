#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]
//! Attribute details tests (NGSI-LD 5.5.11).
//! Uses `?details=true` to retrieve full `Attribute` objects with type and count information.

use reqwest::StatusCode;
use serde_json::Value;

use crate::{
    COMPOUND_CONTEXT_URL,
    building_simple_attributes,
    client,
    create_entity,
    delete_entity,
    expected_attribute_details_compacted,
    expected_attribute_details_expanded,
    list_attributes,
    unique_id,
};
use andromeda_test_harness::tenant_scoped_environment::TenantScopedEnvironment;

fn sort_attributes(val: &Value) -> Vec<Value> {
    let mut arr: Vec<Value> = val.as_array().unwrap().clone();
    arr.sort_by(|a, b| a["attributeName"].as_str().unwrap().cmp(b["attributeName"].as_str().unwrap()));
    arr
}

/// List attribute details without Link header -- attribute names returned as expanded IRIs (5.5.11).
#[tokio::test]
async fn list_attribute_details_without_context_returns_expanded_uris() {
    let env = TenantScopedEnvironment::from_suite("discovery").await.expect("private tenant environment");
    let c = client();
    let id = unique_id("urn:ngsi-ld:Building:attr-details-no-ctx");

    let entity = building_simple_attributes(&id);
    let (s, _) = create_entity(&c, env.tenant(), &entity, Some(COMPOUND_CONTEXT_URL)).await;
    pretty_assertions::assert_eq!(s, StatusCode::CREATED);

    let (status, body) = list_attributes(&c, env.tenant(), "details=true", None).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    let expected = expected_attribute_details_expanded();
    pretty_assertions::assert_eq!(sort_attributes(&body), sort_attributes(&expected));

    delete_entity(&c, env.tenant(), &id).await;
    env.finish().await.expect("cleanup failed");
}

/// List attribute details with compound context Link header -- attribute names compacted (5.5.11).
#[tokio::test]
async fn list_attribute_details_with_context_returns_compacted_names() {
    let env = TenantScopedEnvironment::from_suite("discovery").await.expect("private tenant environment");
    let c = client();
    let id = unique_id("urn:ngsi-ld:Building:attr-details-with-ctx");

    let entity = building_simple_attributes(&id);
    let (s, _) = create_entity(&c, env.tenant(), &entity, Some(COMPOUND_CONTEXT_URL)).await;
    pretty_assertions::assert_eq!(s, StatusCode::CREATED);

    let (status, body) = list_attributes(&c, env.tenant(), "details=true", Some(COMPOUND_CONTEXT_URL)).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    let expected = expected_attribute_details_compacted();
    pretty_assertions::assert_eq!(sort_attributes(&body), sort_attributes(&expected));

    delete_entity(&c, env.tenant(), &id).await;
    env.finish().await.expect("cleanup failed");
}
