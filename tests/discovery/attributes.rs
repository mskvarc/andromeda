#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]
//! Attribute list tests (NGSI-LD 5.5.11, 6.3.5).

use reqwest::StatusCode;
use serde_json::Value;

use crate::{
    COMPOUND_CONTEXT_URL,
    building_simple_attributes,
    client,
    create_entity,
    delete_entity,
    expected_attribute_list_compacted,
    expected_attribute_list_expanded,
    list_attributes,
    provision_tenant,
    unique_id,
};
use andromeda_test_harness::tenant_scoped_environment::TenantScopedEnvironment;

fn sorted_strings(val: &Value) -> Vec<String> {
    let mut v: Vec<String> = val.as_array().unwrap().iter().map(|s| s.as_str().unwrap().to_string()).collect();
    v.sort();
    v
}

// ---------------------------------------------------------------------------
// 5.5.11 -- Empty database
// ---------------------------------------------------------------------------

/// List attributes on empty database -- returns `AttributeList` with empty attributeList (5.5.11).
#[tokio::test]
async fn list_attributes_empty_database_returns_empty_list() {
    let env = TenantScopedEnvironment::from_suite("discovery").await.expect("private tenant environment");
    let c = client();
    provision_tenant(&c, env.tenant()).await;

    let (status, body) = list_attributes(&c, env.tenant(), "", None).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    pretty_assertions::assert_eq!(body["type"], "AttributeList");
    let attrs = body["attributeList"].as_array().expect("attributeList should be array");
    assert!(attrs.is_empty(), "expected empty attributeList on clean database, got: {attrs:?}");
    env.finish().await.expect("cleanup failed");
}

// ---------------------------------------------------------------------------
// 5.5.11, 6.3.5 -- Without Link header context
// ---------------------------------------------------------------------------

/// List attributes without Link header -- attribute URIs returned as expanded (absolute) IRIs (5.5.11, 6.3.5).
#[tokio::test]
async fn list_attributes_without_context_returns_expanded_uris() {
    let env = TenantScopedEnvironment::from_suite("discovery").await.expect("private tenant environment");
    let c = client();
    let id = unique_id("urn:ngsi-ld:Building:attrs-no-ctx");

    let entity = building_simple_attributes(&id);
    let (s, _) = create_entity(&c, env.tenant(), &entity, Some(COMPOUND_CONTEXT_URL)).await;
    pretty_assertions::assert_eq!(s, StatusCode::CREATED);

    let (status, body) = list_attributes(&c, env.tenant(), "", None).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    let expected = expected_attribute_list_expanded();
    pretty_assertions::assert_eq!(body["type"], "AttributeList");
    pretty_assertions::assert_eq!(sorted_strings(&body["attributeList"]), sorted_strings(&expected["attributeList"]));

    delete_entity(&c, env.tenant(), &id).await;
    env.finish().await.expect("cleanup failed");
}

// ---------------------------------------------------------------------------
// 5.5.11, 6.3.5 -- With Link header context
// ---------------------------------------------------------------------------

/// List attributes with compound context Link header -- attribute names compacted to short names (5.5.11, 6.3.5).
#[tokio::test]
async fn list_attributes_with_context_returns_compacted_names() {
    let env = TenantScopedEnvironment::from_suite("discovery").await.expect("private tenant environment");
    let c = client();
    let id = unique_id("urn:ngsi-ld:Building:attrs-with-ctx");

    let entity = building_simple_attributes(&id);
    let (s, _) = create_entity(&c, env.tenant(), &entity, Some(COMPOUND_CONTEXT_URL)).await;
    pretty_assertions::assert_eq!(s, StatusCode::CREATED);

    let (status, body) = list_attributes(&c, env.tenant(), "", Some(COMPOUND_CONTEXT_URL)).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    let expected = expected_attribute_list_compacted();
    pretty_assertions::assert_eq!(body["type"], "AttributeList");
    pretty_assertions::assert_eq!(sorted_strings(&body["attributeList"]), sorted_strings(&expected["attributeList"]));

    delete_entity(&c, env.tenant(), &id).await;
    env.finish().await.expect("cleanup failed");
}
