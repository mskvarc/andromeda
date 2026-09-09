#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]
//! Distributed provision operation tests (create, delete, merge, replace).
//!
//! Tests for distributed entity provisioning across context sources including:
//! - Create entity with different registration modes
//! - Delete entity with different registration modes
//! - Merge entity with different registration modes
//! - Replace entity with different registration modes
//! - Multi-status responses for partial success/failure
//! - local=true parameter to skip forwarding

use andromeda_test_harness::configuration::RUNTIME_BROKER_BASE as BASE_URL;
use reqwest::{Method, StatusCode};
use serde_json::Value;
use serial_test::serial;

use crate::{
    context_source_adapter::ContextSourceAdapter as MockServer,
    helpers::{
        clean_database,
        client,
        create_entity,
        create_entity_local,
        create_registration,
        delete_entity,
        delete_entity_local,
        delete_entity_with_status,
        delete_registration,
        get_entity,
        jsonld_link_value,
        merge_entity,
        query_entities,
        replace_entity,
        unique_id,
        vehicle_entity,
        vehicle_entity_different_ld,
        vehicle_entity_json,
        vehicle_entity_second,
        vehicle_entity_second_ld,
        vehicle_entity_speed_only,
        vehicle_registration,
        vehicle_registration_redirection_ops,
        vehicle_registration_speed_default_ops,
        vehicle_registration_speed_redirection_ops,
        vehicle_replacement_json,
    },
};

// ===========================================================================
// Create Entity Tests
// ===========================================================================

/// Create entity on both context broker and context source (inclusive mode).
#[tokio::test]
#[serial]
async fn distop_entity_create_inclusive_forwards_to_context_source() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let mock = MockServer::start().await;

    let entity_id = unique_id("urn:ngsi-ld:Vehicle:create-inc");
    let reg_id = unique_id("urn:ngsi-ld:Registration:create-inc");

    let reg = vehicle_registration_redirection_ops(&reg_id, Some(&entity_id), &mock.url(""), None);
    let (status, _) = create_registration(&c, &reg).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    mock.set_stub(Method::POST, "/ngsi-ld/v1/entities", 201).await;

    let entity = vehicle_entity(&entity_id);
    let (status, _) = create_entity(&c, &entity).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let count = mock.get_count(Method::POST, "/ngsi-ld/v1/entities");
    assert!(count > 0, "forwarding should have happened");

    let (status, _, body) = query_entities(&c, "type=Vehicle", &jsonld_link_value()).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    let entities = body.as_array().expect("response should be array");
    assert!(entities.iter().any(|e| e["id"] == entity_id), "entity should exist locally");

    delete_entity(&c, &entity_id).await;
    delete_registration(&c, &reg_id).await;
    mock.shutdown().await;
}

/// Create entity with exclusive registration forwards only exclusive attributes.
#[tokio::test]
#[serial]
async fn distop_entity_create_exclusive_forwards_exclusive_attributes() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let mock = MockServer::start().await;

    let entity_id = unique_id("urn:ngsi-ld:Vehicle:create-exc");
    let reg_id = unique_id("urn:ngsi-ld:Registration:create-exc");

    // Create entity locally first
    let entity = vehicle_entity(&entity_id);
    let (status, _) = create_entity_local(&c, &entity).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    // Create exclusive registration for speed attribute
    let reg = vehicle_registration_speed_redirection_ops(&reg_id, &entity_id, &mock.url("/broker1"), Some("exclusive"));
    let (status, _) = create_registration(&c, &reg).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    mock.set_stub(Method::POST, "/broker1/ngsi-ld/v1/entities", 201).await;

    // Create entity with speed attribute
    let speed_entity = vehicle_entity_speed_only(&entity_id);
    let (status, _) = create_entity(&c, &speed_entity).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let count = mock.get_count(Method::POST, "/broker1/ngsi-ld/v1/entities");
    assert!(count > 0, "forwarding should have happened");

    // Local entity should NOT have speed (exclusive)
    let (status, _, body) = get_entity(&c, &entity_id, "local=true", &jsonld_link_value()).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    assert!(!body.as_object().unwrap().contains_key("speed"), "speed should not be local");

    delete_entity(&c, &entity_id).await;
    delete_registration(&c, &reg_id).await;
    mock.shutdown().await;
}

/// Create entities with redirect registrations forwards to multiple context sources.
#[tokio::test]
#[serial]
async fn distop_entity_create_redirect_forwards_to_multiple_sources() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let mock = MockServer::start().await;

    let entity_id1 = unique_id("urn:ngsi-ld:Vehicle:create-red-1");
    let entity_id2 = unique_id("urn:ngsi-ld:Vehicle:create-red-2");
    let reg_id1 = unique_id("urn:ngsi-ld:Registration:create-red-1");
    let reg_id2 = unique_id("urn:ngsi-ld:Registration:create-red-2");

    let reg1 = vehicle_registration_redirection_ops(&reg_id1, Some(&entity_id1), &mock.url("/broker1"), Some("redirect"));
    let (status, _) = create_registration(&c, &reg1).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let reg2 = vehicle_registration_redirection_ops(&reg_id2, Some(&entity_id2), &mock.url("/broker2"), Some("redirect"));
    let (status, _) = create_registration(&c, &reg2).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    mock.set_stub(Method::POST, "/broker1/ngsi-ld/v1/entities", 201).await;
    mock.set_stub(Method::POST, "/broker2/ngsi-ld/v1/entities", 201).await;

    let entity1 = vehicle_entity(&entity_id1);
    let (status, _) = create_entity(&c, &entity1).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);
    assert!(mock.get_count(Method::POST, "/broker1/ngsi-ld/v1/entities") > 0);

    let entity2 = vehicle_entity(&entity_id2);
    let (status, _) = create_entity(&c, &entity2).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);
    assert!(mock.get_count(Method::POST, "/broker2/ngsi-ld/v1/entities") > 0);

    delete_registration(&c, &reg_id1).await;
    delete_registration(&c, &reg_id2).await;
    mock.shutdown().await;
}

/// Create entity without redirection operations should not forward.
#[tokio::test]
#[serial]
async fn distop_entity_create_without_redirection_ops_no_forwarding() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let mock = MockServer::start().await;

    let entity_id = unique_id("urn:ngsi-ld:Vehicle:create-no-ops");
    let reg_id = unique_id("urn:ngsi-ld:Registration:create-no-ops");

    // Create entity locally first
    let entity = vehicle_entity(&entity_id);
    let (status, _) = create_entity_local(&c, &entity).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    // Create exclusive registration WITHOUT redirection ops
    let reg = vehicle_registration_speed_default_ops(&reg_id, &entity_id, &mock.url("/broker1"), Some("exclusive"));
    let (status, _) = create_registration(&c, &reg).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    mock.set_stub(Method::POST, "/broker1/ngsi-ld/v1/entities", 409).await;

    // Try to create entity with speed -> should fail with 409 (already exists locally)
    let speed_entity = vehicle_entity_speed_only(&entity_id);
    let (status, _) = create_entity(&c, &speed_entity).await;
    pretty_assertions::assert_eq!(status, StatusCode::CONFLICT);

    // No forwarding should have happened
    pretty_assertions::assert_eq!(mock.get_count(Method::POST, "/broker1/ngsi-ld/v1/entities"), 0);

    delete_entity(&c, &entity_id).await;
    delete_registration(&c, &reg_id).await;
    mock.shutdown().await;
}

/// Create entity with malformed ID returns 400.
#[tokio::test]
#[serial]
async fn distop_entity_create_malformed_id_returns_400() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let mock = MockServer::start().await;

    let reg_id = unique_id("urn:ngsi-ld:Registration:create-malformed");

    let reg = vehicle_registration_redirection_ops(&reg_id, None, &mock.url(""), None);
    let (status, _) = create_registration(&c, &reg).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let mut entity = vehicle_entity("InvalidUriExample");
    entity["id"] = serde_json::json!("InvalidUriExample");
    let (status, _) = create_entity(&c, &entity).await;
    pretty_assertions::assert_eq!(status, StatusCode::BAD_REQUEST);

    // No entity should exist
    let (status, _, body) = query_entities(&c, "type=Vehicle", &jsonld_link_value()).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    let entities = body.as_array().expect("response should be array");
    assert!(entities.is_empty(), "a rejected create leaves no entity behind: {entities:?}");

    delete_registration(&c, &reg_id).await;
    mock.shutdown().await;
}

/// Create entity already existing locally returns 207 (local conflict, remote success).
#[tokio::test]
#[serial]
async fn distop_entity_create_existing_locally_returns_207() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let mock = MockServer::start().await;

    let entity_id = unique_id("urn:ngsi-ld:Vehicle:create-exist-local");
    let reg_id = unique_id("urn:ngsi-ld:Registration:create-exist-local");

    // Create entity first
    let entity = vehicle_entity(&entity_id);
    let (status, _) = create_entity(&c, &entity).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    // Create inclusive registration
    let reg = vehicle_registration_redirection_ops(&reg_id, Some(&entity_id), &mock.url(""), None);
    let (status, _) = create_registration(&c, &reg).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    mock.set_stub(Method::POST, "/ngsi-ld/v1/entities", 201).await;

    // Create same entity again -> 207 (local conflict, remote success)
    let entity = vehicle_entity(&entity_id);
    let resp = c
        .post(format!("{BASE_URL}/entities"))
        .header("Content-Type", "application/ld+json")
        .json(&entity)
        .send()
        .await
        .expect("request failed");
    pretty_assertions::assert_eq!(resp.status(), StatusCode::MULTI_STATUS);

    let body: Value = resp.json().await.unwrap();
    let errors = body["errors"].as_array().unwrap();
    pretty_assertions::assert_eq!(errors.len(), 1);
    pretty_assertions::assert_eq!(errors[0]["error"]["status"], 409);

    // Forwarding should still have happened
    assert!(mock.get_count(Method::POST, "/ngsi-ld/v1/entities") > 0);

    delete_entity(&c, &entity_id).await;
    delete_registration(&c, &reg_id).await;
    mock.shutdown().await;
}

/// Create entity that already exists both locally and at the registered
/// context source returns 409 `AlreadyExists`.
///
/// Clause 5.6.1.4: "If the Entity already exists locally this shall result in
/// an error of type `AlreadyExists`, if the complete Create Entity operation has
/// failed or in a partial success if some parts of it has succeeded." No part
/// succeeds here -- the local entity exists and the only matching source also
/// answers 409 -- so the complete operation failed and the error is raised
/// instead of a 207 Multi-Status.
#[tokio::test]
#[serial]
async fn distop_entity_create_existing_on_both_returns_409() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let mock = MockServer::start().await;

    let entity_id = unique_id("urn:ngsi-ld:Vehicle:create-exist-both");
    let reg_id = unique_id("urn:ngsi-ld:Registration:create-exist-both");

    // Create entity locally first
    let entity = vehicle_entity(&entity_id);
    let (status, _) = create_entity(&c, &entity).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let reg = vehicle_registration_redirection_ops(&reg_id, Some(&entity_id), &mock.url(""), None);
    let (status, _) = create_registration(&c, &reg).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    // Remote also returns conflict
    mock.set_stub(Method::POST, "/ngsi-ld/v1/entities", 409).await;

    // Create same entity again -> 409, the complete create having failed
    let entity = vehicle_entity(&entity_id);
    let resp = c
        .post(format!("{BASE_URL}/entities"))
        .header("Content-Type", "application/ld+json")
        .json(&entity)
        .send()
        .await
        .expect("request failed");
    pretty_assertions::assert_eq!(resp.status(), StatusCode::CONFLICT);

    let body: Value = resp.json().await.unwrap();
    assert!(
        body["type"].as_str().unwrap_or_default().contains("AlreadyExists"),
        "the complete create failed, so AlreadyExists is raised: {body}"
    );

    // The create was still forwarded to the matching inclusive registration.
    assert!(mock.get_count(Method::POST, "/ngsi-ld/v1/entities") > 0);

    delete_entity(&c, &entity_id).await;
    delete_registration(&c, &reg_id).await;
    mock.shutdown().await;
}

/// Create entity already existing remotely returns 207 (local success, remote conflict).
#[tokio::test]
#[serial]
async fn distop_entity_create_existing_remotely_returns_207() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let mock = MockServer::start().await;

    let entity_id = unique_id("urn:ngsi-ld:Vehicle:create-exist-remote");
    let reg_id = unique_id("urn:ngsi-ld:Registration:create-exist-remote");

    // Create inclusive registration (no local entity)
    let reg = vehicle_registration_redirection_ops(&reg_id, Some(&entity_id), &mock.url(""), None);
    let (status, _) = create_registration(&c, &reg).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    // Remote returns conflict
    mock.set_stub(Method::POST, "/ngsi-ld/v1/entities", 409).await;

    // Create entity -> 207 (local success, remote conflict)
    let entity = vehicle_entity(&entity_id);
    let resp = c
        .post(format!("{BASE_URL}/entities"))
        .header("Content-Type", "application/ld+json")
        .json(&entity)
        .send()
        .await
        .expect("request failed");
    pretty_assertions::assert_eq!(resp.status(), StatusCode::MULTI_STATUS);

    let body: Value = resp.json().await.unwrap();
    let errors = body["errors"].as_array().unwrap();
    assert!(errors.iter().any(|e| e["error"]["status"] == 409));

    // Entity should exist locally
    let (status, _, body) = query_entities(&c, "type=Vehicle", &jsonld_link_value()).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    let entities = body.as_array().expect("response should be array");
    assert!(entities.iter().any(|e| e["id"] == entity_id));

    delete_entity(&c, &entity_id).await;
    delete_registration(&c, &reg_id).await;
    mock.shutdown().await;
}

/// Create entity with local=true skips forwarding.
#[tokio::test]
#[serial]
async fn distop_entity_create_local_true_skips_forwarding() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let mock = MockServer::start().await;

    let entity_id = unique_id("urn:ngsi-ld:Vehicle:create-local");
    let reg_id = unique_id("urn:ngsi-ld:Registration:create-local");

    let reg = vehicle_registration_redirection_ops(&reg_id, Some(&entity_id), &mock.url(""), None);
    let (status, _) = create_registration(&c, &reg).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let entity = vehicle_entity(&entity_id);
    let (status, _) = create_entity_local(&c, &entity).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    // No forwarding should have happened
    pretty_assertions::assert_eq!(mock.get_total_count(), 0);

    delete_entity(&c, &entity_id).await;
    delete_registration(&c, &reg_id).await;
    mock.shutdown().await;
}

// ===========================================================================
// Delete Entity Tests
// ===========================================================================

/// Delete entity on both context broker and context source (inclusive mode).
#[tokio::test]
#[serial]
async fn distop_entity_delete_inclusive_forwards_to_context_source() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let mock = MockServer::start().await;

    let entity_id = unique_id("urn:ngsi-ld:Vehicle:delete-inc");
    let reg_id = unique_id("urn:ngsi-ld:Registration:delete-inc");

    let entity = vehicle_entity(&entity_id);
    let (status, _) = create_entity(&c, &entity).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let reg = vehicle_registration_redirection_ops(&reg_id, Some(&entity_id), &mock.url(""), None);
    let (status, _) = create_registration(&c, &reg).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let delete_path = format!("/ngsi-ld/v1/entities/{entity_id}");
    mock.set_stub(Method::DELETE, &delete_path, 204).await;

    let (status, _) = delete_entity_with_status(&c, &entity_id).await;
    pretty_assertions::assert_eq!(status, StatusCode::NO_CONTENT);

    assert!(mock.get_count(Method::DELETE, &delete_path) > 0);

    // Entity should be gone locally
    let (status, _, _) = get_entity(&c, &entity_id, "local=true", &jsonld_link_value()).await;
    pretty_assertions::assert_eq!(status, StatusCode::NOT_FOUND);

    delete_registration(&c, &reg_id).await;
    mock.shutdown().await;
}

/// Delete entity with exclusive registration forwards to context source.
#[tokio::test]
#[serial]
async fn distop_entity_delete_exclusive_forwards_to_context_source() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let mock = MockServer::start().await;

    let entity_id = unique_id("urn:ngsi-ld:Vehicle:delete-exc");
    let reg_id = unique_id("urn:ngsi-ld:Registration:delete-exc");

    let entity = vehicle_entity(&entity_id);
    let (status, _) = create_entity_local(&c, &entity).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let reg = vehicle_registration_speed_redirection_ops(&reg_id, &entity_id, &mock.url("/broker1"), Some("exclusive"));
    let (status, _) = create_registration(&c, &reg).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let delete_path = format!("/broker1/ngsi-ld/v1/entities/{entity_id}");
    mock.set_stub(Method::DELETE, &delete_path, 204).await;

    let (status, _) = delete_entity_with_status(&c, &entity_id).await;
    pretty_assertions::assert_eq!(status, StatusCode::NO_CONTENT);

    assert!(mock.get_count(Method::DELETE, &delete_path) > 0);

    delete_registration(&c, &reg_id).await;
    mock.shutdown().await;
}

/// Delete entities with redirect registrations forwards to multiple context sources.
#[tokio::test]
#[serial]
async fn distop_entity_delete_redirect_forwards_to_multiple_sources() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let mock = MockServer::start().await;

    let entity_id1 = unique_id("urn:ngsi-ld:Vehicle:delete-red-1");
    let entity_id2 = unique_id("urn:ngsi-ld:Vehicle:delete-red-2");
    let reg_id1 = unique_id("urn:ngsi-ld:Registration:delete-red-1");
    let reg_id2 = unique_id("urn:ngsi-ld:Registration:delete-red-2");

    let reg1 = vehicle_registration_redirection_ops(&reg_id1, Some(&entity_id1), &mock.url("/broker1"), Some("redirect"));
    let (status, _) = create_registration(&c, &reg1).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let reg2 = vehicle_registration_redirection_ops(&reg_id2, Some(&entity_id2), &mock.url("/broker2"), Some("redirect"));
    let (status, _) = create_registration(&c, &reg2).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    // Create entities via redirect
    mock.set_stub(Method::POST, "/broker1/ngsi-ld/v1/entities", 201).await;
    mock.set_stub(Method::POST, "/broker2/ngsi-ld/v1/entities", 201).await;

    let entity1 = vehicle_entity(&entity_id1);
    let (status, _) = create_entity(&c, &entity1).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let entity2 = vehicle_entity(&entity_id2);
    let (status, _) = create_entity(&c, &entity2).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    // Set up delete stubs
    let delete_path1 = format!("/broker1/ngsi-ld/v1/entities/{entity_id1}");
    let delete_path2 = format!("/broker2/ngsi-ld/v1/entities/{entity_id2}");
    mock.set_stub(Method::DELETE, &delete_path1, 204).await;
    mock.set_stub(Method::DELETE, &delete_path2, 204).await;

    let (status, _) = delete_entity_with_status(&c, &entity_id1).await;
    pretty_assertions::assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, _) = delete_entity_with_status(&c, &entity_id2).await;
    pretty_assertions::assert_eq!(status, StatusCode::NO_CONTENT);

    assert!(mock.get_count(Method::DELETE, &delete_path1) > 0);
    assert!(mock.get_count(Method::DELETE, &delete_path2) > 0);

    delete_registration(&c, &reg_id1).await;
    delete_registration(&c, &reg_id2).await;
    mock.shutdown().await;
}

/// Delete entity: local success, remote failure returns 207.
#[tokio::test]
#[serial]
async fn distop_entity_delete_local_success_remote_failure_returns_207() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let mock = MockServer::start().await;

    let entity_id = unique_id("urn:ngsi-ld:Vehicle:delete-local-ok");
    let reg_id = unique_id("urn:ngsi-ld:Registration:delete-local-ok");

    let entity = vehicle_entity(&entity_id);
    let (status, _) = create_entity(&c, &entity).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let reg = vehicle_registration_redirection_ops(&reg_id, Some(&entity_id), &mock.url(""), None);
    let (status, _) = create_registration(&c, &reg).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let delete_path = format!("/ngsi-ld/v1/entities/{entity_id}");
    mock.set_stub(Method::DELETE, &delete_path, 404).await;

    let (status, body) = delete_entity_with_status(&c, &entity_id).await;
    pretty_assertions::assert_eq!(status, StatusCode::MULTI_STATUS);

    pretty_assertions::assert_eq!(body["errors"][0]["error"]["status"], 404);

    // Entity should be gone locally
    let (status, _, _) = get_entity(&c, &entity_id, "local=true", &jsonld_link_value()).await;
    pretty_assertions::assert_eq!(status, StatusCode::NOT_FOUND);

    delete_registration(&c, &reg_id).await;
    mock.shutdown().await;
}

/// Delete entity: local failure, remote success returns 207.
#[tokio::test]
#[serial]
async fn distop_entity_delete_local_failure_remote_success_returns_207() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let mock = MockServer::start().await;

    let entity_id = unique_id("urn:ngsi-ld:Vehicle:delete-remote-ok");
    let reg_id = unique_id("urn:ngsi-ld:Registration:delete-remote-ok");

    // No local entity, only registration
    let reg = vehicle_registration_redirection_ops(&reg_id, Some(&entity_id), &mock.url(""), None);
    let (status, _) = create_registration(&c, &reg).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let delete_path = format!("/ngsi-ld/v1/entities/{entity_id}");
    mock.set_stub(Method::DELETE, &delete_path, 204).await;

    let (status, body) = delete_entity_with_status(&c, &entity_id).await;
    pretty_assertions::assert_eq!(status, StatusCode::MULTI_STATUS);

    let errors = body["errors"].as_array().unwrap();
    pretty_assertions::assert_eq!(errors.len(), 1);
    pretty_assertions::assert_eq!(errors[0]["error"]["status"], 404);

    delete_registration(&c, &reg_id).await;
    mock.shutdown().await;
}

/// Exclusive ownership without delete capability returns a conflict.
#[tokio::test]
#[serial]
async fn distop_entity_delete_exclusive_without_operation_returns_conflict() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let mock = MockServer::start().await;

    let entity_id = unique_id("urn:ngsi-ld:Vehicle:delete-no-ops");
    let reg_id = unique_id("urn:ngsi-ld:Registration:delete-no-ops");

    let entity = vehicle_entity(&entity_id);
    let (status, _) = create_entity_local(&c, &entity).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    // Registration without redirection ops
    let reg = vehicle_registration_speed_default_ops(&reg_id, &entity_id, &mock.url("/broker1"), Some("exclusive"));
    let (status, _) = create_registration(&c, &reg).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let delete_path = format!("/broker1/ngsi-ld/v1/entities/{entity_id}");
    mock.set_stub(Method::DELETE, &delete_path, 207).await;

    let (status, _) = delete_entity_with_status(&c, &entity_id).await;
    pretty_assertions::assert_eq!(status, StatusCode::CONFLICT);

    // No forwarding should have happened
    pretty_assertions::assert_eq!(mock.get_count(Method::DELETE, &delete_path), 0);

    delete_registration(&c, &reg_id).await;
    mock.shutdown().await;
}

/// Delete entity with local=true skips forwarding.
#[tokio::test]
#[serial]
async fn distop_entity_delete_local_true_skips_forwarding() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let mock = MockServer::start().await;

    let entity_id = unique_id("urn:ngsi-ld:Vehicle:delete-local");
    let reg_id = unique_id("urn:ngsi-ld:Registration:delete-local");

    let entity = vehicle_entity(&entity_id);
    let (status, _) = create_entity(&c, &entity).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let reg = vehicle_registration_redirection_ops(&reg_id, Some(&entity_id), &mock.url(""), None);
    let (status, _) = create_registration(&c, &reg).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let status = delete_entity_local(&c, &entity_id).await;
    pretty_assertions::assert_eq!(status, StatusCode::NO_CONTENT);

    // No forwarding should have happened
    pretty_assertions::assert_eq!(mock.get_total_count(), 0);

    delete_registration(&c, &reg_id).await;
    mock.shutdown().await;
}

// ===========================================================================
// Merge Entity Tests
// ===========================================================================

/// Merge entity with exclusive registration forwards exclusive attributes.
#[tokio::test]
#[serial]
async fn distop_entity_merge_exclusive_forwards_exclusive_attributes() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let mock = MockServer::start().await;

    let entity_id = unique_id("urn:ngsi-ld:Vehicle:merge-exc");
    let reg_id = unique_id("urn:ngsi-ld:Registration:merge-exc");

    // Create entity locally
    let entity = vehicle_entity_second_ld(&entity_id);
    let (status, _) = create_entity_local(&c, &entity).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    // Create exclusive registration for speed
    let reg = vehicle_registration_speed_redirection_ops(&reg_id, &entity_id, &mock.url("/broker1"), Some("exclusive"));
    let (status, _) = create_registration(&c, &reg).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let patch_path = format!("/broker1/ngsi-ld/v1/entities/{entity_id}");
    mock.set_stub(Method::PATCH, &patch_path, 204).await;

    // Merge entity with different attributes including speed
    let merge_payload = vehicle_entity_different_ld(&entity_id);
    let (status, _) = merge_entity(&c, &entity_id, &merge_payload, &jsonld_link_value()).await;
    pretty_assertions::assert_eq!(status, StatusCode::NO_CONTENT);

    assert!(mock.get_count(Method::PATCH, &patch_path) > 0);

    // Local entity should NOT have speed
    let (status, _, body) = get_entity(&c, &entity_id, "local=true", &jsonld_link_value()).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    assert!(!body.as_object().unwrap().contains_key("speed"), "speed should not be local");

    delete_entity(&c, &entity_id).await;
    delete_registration(&c, &reg_id).await;
    mock.shutdown().await;
}

/// Merge entity with inclusive registration updates both.
#[tokio::test]
#[serial]
async fn distop_entity_merge_inclusive_updates_local_and_forwards() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let mock = MockServer::start().await;

    let entity_id = unique_id("urn:ngsi-ld:Vehicle:merge-inc");
    let reg_id = unique_id("urn:ngsi-ld:Registration:merge-inc");

    let entity = vehicle_entity_second_ld(&entity_id);
    let (status, _) = create_entity(&c, &entity).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let reg = vehicle_registration_redirection_ops(&reg_id, Some(&entity_id), &mock.url(""), Some("inclusive"));
    let (status, _) = create_registration(&c, &reg).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    // Get old entity state
    let (status, _, old_body) = get_entity(&c, &entity_id, "", &jsonld_link_value()).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    let old_isparked2 = old_body["isParked2"].clone();
    let old_brandname = old_body["brandName"].clone();

    let patch_path = format!("/ngsi-ld/v1/entities/{entity_id}");
    mock.set_stub(Method::PATCH, &patch_path, 204).await;

    // Merge with different attributes
    let merge_payload = vehicle_entity_different_ld(&entity_id);
    let (status, _) = merge_entity(&c, &entity_id, &merge_payload, &jsonld_link_value()).await;
    pretty_assertions::assert_eq!(status, StatusCode::NO_CONTENT);

    assert!(mock.get_count(Method::PATCH, &patch_path) > 0);

    // Entity should be updated locally
    let (status, _, new_body) = get_entity(&c, &entity_id, "", &jsonld_link_value()).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    pretty_assertions::assert_eq!(old_brandname, new_body["brandName"]); // unchanged
    pretty_assertions::assert_ne!(old_isparked2, new_body["isParked2"]); // changed
    assert!(new_body.as_object().unwrap().contains_key("speed")); // added

    delete_entity(&c, &entity_id).await;
    delete_registration(&c, &reg_id).await;
    mock.shutdown().await;
}

/// Merge entity with redirect registrations forwards to multiple sources.
#[tokio::test]
#[serial]
async fn distop_entity_merge_redirect_forwards_to_multiple_sources() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let mock = MockServer::start().await;

    let entity_id = unique_id("urn:ngsi-ld:Vehicle:merge-red");
    let reg_id1 = unique_id("urn:ngsi-ld:Registration:merge-red-1");
    let reg_id2 = unique_id("urn:ngsi-ld:Registration:merge-red-2");

    // Two redirect registrations for same entity
    let reg1 = vehicle_registration_redirection_ops(&reg_id1, Some(&entity_id), &mock.url("/broker1"), Some("redirect"));
    let (status, _) = create_registration(&c, &reg1).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let reg2 = vehicle_registration_redirection_ops(&reg_id2, Some(&entity_id), &mock.url("/broker2"), Some("redirect"));
    let (status, _) = create_registration(&c, &reg2).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let patch_path1 = format!("/broker1/ngsi-ld/v1/entities/{entity_id}");
    let patch_path2 = format!("/broker2/ngsi-ld/v1/entities/{entity_id}");
    mock.set_stub(Method::PATCH, &patch_path1, 204).await;
    mock.set_stub(Method::PATCH, &patch_path2, 204).await;

    let merge_payload = vehicle_entity_different_ld(&entity_id);
    let (status, _) = merge_entity(&c, &entity_id, &merge_payload, &jsonld_link_value()).await;
    pretty_assertions::assert_eq!(status, StatusCode::NO_CONTENT);

    assert!(mock.get_count(Method::PATCH, &patch_path1) > 0);
    assert!(mock.get_count(Method::PATCH, &patch_path2) > 0);

    delete_registration(&c, &reg_id1).await;
    delete_registration(&c, &reg_id2).await;
    mock.shutdown().await;
}

// ===========================================================================
// Replace Entity Tests
// ===========================================================================

/// Replace entity with exclusive registration forwards exclusive attributes.
#[tokio::test]
#[serial]
async fn distop_entity_replace_exclusive_forwards_exclusive_attributes() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let mock = MockServer::start().await;

    let entity_id = unique_id("urn:ngsi-ld:Vehicle:replace-exc");
    let reg_id = unique_id("urn:ngsi-ld:Registration:replace-exc");

    // Create entity locally
    let entity = vehicle_entity(&entity_id);
    let (status, _) = create_entity_local(&c, &entity).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    // Create exclusive registration for speed
    let reg = vehicle_registration_speed_redirection_ops(&reg_id, &entity_id, &mock.url("/broker1"), Some("exclusive"));
    let (status, _) = create_registration(&c, &reg).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let put_path = format!("/broker1/ngsi-ld/v1/entities/{entity_id}");
    mock.set_stub(Method::PUT, &put_path, 204).await;

    // Replace entity
    let replacement = vehicle_replacement_json(&entity_id);
    let (status, _) = replace_entity(&c, &entity_id, &replacement, &jsonld_link_value()).await;
    pretty_assertions::assert_eq!(status, StatusCode::NO_CONTENT);

    assert!(mock.get_count(Method::PUT, &put_path) > 0);

    // Local entity should NOT have speed (exclusive)
    let (status, _, body) = get_entity(&c, &entity_id, "local=true", &jsonld_link_value()).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    assert!(!body.as_object().unwrap().contains_key("speed"), "speed should not be local");

    delete_entity(&c, &entity_id).await;
    delete_registration(&c, &reg_id).await;
    mock.shutdown().await;
}

/// Replace entity with inclusive registration replaces both.
#[tokio::test]
#[serial]
async fn distop_entity_replace_inclusive_replaces_local_and_forwards() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let mock = MockServer::start().await;

    let entity_id = unique_id("urn:ngsi-ld:Vehicle:replace-inc");
    let reg_id = unique_id("urn:ngsi-ld:Registration:replace-inc");

    // Create entity with JSON content type
    let entity = vehicle_entity_json(&entity_id);
    let resp = c
        .post(format!("{BASE_URL}/entities"))
        .header("Content-Type", "application/json")
        .header("Link", jsonld_link_value())
        .json(&entity)
        .send()
        .await
        .expect("request failed");
    pretty_assertions::assert_eq!(resp.status(), StatusCode::CREATED);

    let reg = vehicle_registration_redirection_ops(&reg_id, Some(&entity_id), &mock.url(""), Some("inclusive"));
    let (status, _) = create_registration(&c, &reg).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    // Get old entity state
    let (status, _, old_body) = get_entity(&c, &entity_id, "", &jsonld_link_value()).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    let put_path = format!("/ngsi-ld/v1/entities/{entity_id}");
    mock.set_stub(Method::PUT, &put_path, 204).await;

    // Replace entity
    let replacement = vehicle_entity_second(&entity_id);
    let (status, _) = replace_entity(&c, &entity_id, &replacement, &jsonld_link_value()).await;
    pretty_assertions::assert_eq!(status, StatusCode::NO_CONTENT);

    // Entity body should have changed
    let (status, _, new_body) = get_entity(&c, &entity_id, "", &jsonld_link_value()).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    pretty_assertions::assert_ne!(old_body, new_body);

    delete_entity(&c, &entity_id).await;
    delete_registration(&c, &reg_id).await;
    mock.shutdown().await;
}

/// Replace entity with redirect registrations forwards to multiple sources.
#[tokio::test]
#[serial]
async fn distop_entity_replace_redirect_forwards_to_multiple_sources() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let mock = MockServer::start().await;

    let entity_id = unique_id("urn:ngsi-ld:Vehicle:replace-red");
    let reg_id1 = unique_id("urn:ngsi-ld:Registration:replace-red-1");
    let reg_id2 = unique_id("urn:ngsi-ld:Registration:replace-red-2");

    // Two redirect registrations for same entity
    let reg1 = vehicle_registration_redirection_ops(&reg_id1, Some(&entity_id), &mock.url("/broker1"), Some("redirect"));
    let (status, _) = create_registration(&c, &reg1).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let reg2 = vehicle_registration_redirection_ops(&reg_id2, Some(&entity_id), &mock.url("/broker2"), Some("redirect"));
    let (status, _) = create_registration(&c, &reg2).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let put_path1 = format!("/broker1/ngsi-ld/v1/entities/{entity_id}");
    let put_path2 = format!("/broker2/ngsi-ld/v1/entities/{entity_id}");
    mock.set_stub(Method::PUT, &put_path1, 204).await;
    mock.set_stub(Method::PUT, &put_path2, 204).await;

    let replacement = vehicle_entity_second(&entity_id);
    let (status, _) = replace_entity(&c, &entity_id, &replacement, &jsonld_link_value()).await;
    pretty_assertions::assert_eq!(status, StatusCode::NO_CONTENT);

    pretty_assertions::assert_eq!(mock.get_count(Method::PUT, &put_path1), 1);
    pretty_assertions::assert_eq!(mock.get_count(Method::PUT, &put_path2), 1);

    delete_registration(&c, &reg_id1).await;
    delete_registration(&c, &reg_id2).await;
    mock.shutdown().await;
}

// ===========================================================================
// Additional Tests (Extended Coverage)
// ===========================================================================

/// Create entity with auxiliary registration creates locally only.
#[tokio::test]
#[serial]
async fn distop_entity_create_auxiliary_creates_locally_only() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let mock = MockServer::start().await;

    let entity_id = unique_id("urn:ngsi-ld:Vehicle:create-aux");
    let reg_id = unique_id("urn:ngsi-ld:Registration:create-aux");

    // Clause 5.9.2.4: an auxiliary registration whose operations are not
    // retrieveOps, retrieveEntity or queryEntity is rejected with
    // BadRequestData, so the source is registered with the default operations.
    let reg = vehicle_registration(&reg_id, Some(&entity_id), &mock.url(""), Some("auxiliary"));
    let (status, _) = create_registration(&c, &reg).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    mock.set_stub(Method::POST, "/ngsi-ld/v1/entities", 201).await;

    let entity = vehicle_entity(&entity_id);
    let (status, _) = create_entity(&c, &entity).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    // Auxiliary should not forward provision operations
    pretty_assertions::assert_eq!(mock.get_count(Method::POST, "/ngsi-ld/v1/entities"), 0);

    delete_entity(&c, &entity_id).await;
    delete_registration(&c, &reg_id).await;
    mock.shutdown().await;
}

/// Delete entity with auxiliary registration deletes locally only.
#[tokio::test]
#[serial]
async fn distop_entity_delete_auxiliary_deletes_locally_only() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let mock = MockServer::start().await;

    let entity_id = unique_id("urn:ngsi-ld:Vehicle:delete-aux");
    let reg_id = unique_id("urn:ngsi-ld:Registration:delete-aux");

    let entity = vehicle_entity(&entity_id);
    let (status, _) = create_entity(&c, &entity).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    // Clause 5.9.2.4: an auxiliary registration whose operations are not
    // retrieveOps, retrieveEntity or queryEntity is rejected with
    // BadRequestData, so the source is registered with the default operations.
    let reg = vehicle_registration(&reg_id, Some(&entity_id), &mock.url(""), Some("auxiliary"));
    let (status, _) = create_registration(&c, &reg).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let delete_path = format!("/ngsi-ld/v1/entities/{entity_id}");
    mock.set_stub(Method::DELETE, &delete_path, 204).await;

    let (status, _) = delete_entity_with_status(&c, &entity_id).await;
    pretty_assertions::assert_eq!(status, StatusCode::NO_CONTENT);

    // Auxiliary should not forward provision operations
    pretty_assertions::assert_eq!(mock.get_count(Method::DELETE, &delete_path), 0);

    delete_registration(&c, &reg_id).await;
    mock.shutdown().await;
}

/// Merge entity not found returns 404.
#[tokio::test]
#[serial]
async fn distop_entity_merge_not_found_returns_404() {
    let _serialized_environment = clean_database().await;
    let c = client();

    let entity_id = unique_id("urn:ngsi-ld:Vehicle:merge-notfound");

    let merge_payload = vehicle_entity_different_ld(&entity_id);
    let (status, _) = merge_entity(&c, &entity_id, &merge_payload, &jsonld_link_value()).await;
    pretty_assertions::assert_eq!(status, StatusCode::NOT_FOUND);
}

/// Replace entity not found returns 404.
#[tokio::test]
#[serial]
async fn distop_entity_replace_not_found_returns_404() {
    let _serialized_environment = clean_database().await;
    let c = client();

    let entity_id = unique_id("urn:ngsi-ld:Vehicle:replace-notfound");

    let replacement = vehicle_replacement_json(&entity_id);
    let (status, _) = replace_entity(&c, &entity_id, &replacement, &jsonld_link_value()).await;
    pretty_assertions::assert_eq!(status, StatusCode::NOT_FOUND);
}
