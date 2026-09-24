#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]
//! Append entity attributes distributed operation tests.
//!
//! Tests for POST /entities/{entityId}/attrs with different registration modes.

use reqwest::{Method, StatusCode};

use crate::{
    context_source_adapter::ContextSourceAdapter as MockServer,
    helpers::{
        append_entity_attrs,
        append_entity_attrs_no_overwrite,
        client,
        create_entity,
        create_entity_local,
        create_registration,
        delete_entity,
        delete_registration,
        fragment_speed_isparked,
        get_entity,
        jsonld_link_value,
        vehicle_entity,
        vehicle_registration_auxiliary_retrieve_ops,
        vehicle_registration_redirection_ops,
        vehicle_registration_speed_redirection_ops,
    },
};
use andromeda_test_harness::environment::TestEnvironment;

// ---------------------------------------------------------------------------
// Append entity attributes - inclusive mode
// ---------------------------------------------------------------------------

/// Append with inclusive registration: updates local entity and forwards to context source.
#[tokio::test]
async fn distop_entity_attrs_append_inclusive_forwards_and_updates_local() {
    let scope = TestEnvironment::from_suite("distop_entity_attributes")
        .await
        .expect("isolated test environment");
    let c = client();
    let mock = MockServer::start().await;

    let entity_id = scope.id("Vehicle:append-inc");
    let reg_id = scope.id("Registration:append-inc");

    // Create entity locally
    let entity = vehicle_entity(&entity_id);
    let (status, _) = create_entity(&c, &entity).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    // Get original isParked value
    let (status, _, old_body) = get_entity(&c, &entity_id, "", &jsonld_link_value()).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    let old_is_parked = old_body["isParked"].clone();

    // Create inclusive registration
    let reg = vehicle_registration_redirection_ops(&reg_id, Some(&entity_id), &mock.url(""), Some("inclusive"));
    let (status, _) = create_registration(&c, &reg).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let stub_path = format!("/ngsi-ld/v1/entities/{entity_id}/attrs");
    mock.set_stub(Method::POST, &stub_path, 204).await;

    // Append attributes
    let fragment = fragment_speed_isparked();
    let status = append_entity_attrs(&c, &entity_id, &fragment, &jsonld_link_value()).await;
    pretty_assertions::assert_eq!(status, StatusCode::NO_CONTENT);

    // Verify forwarding happened
    let count = mock.get_count(Method::POST, &stub_path);
    pretty_assertions::assert_eq!(count, 1, "forwarding should have happened");

    // Verify local entity has speed and isParked changed
    let (status, _, body) = get_entity(&c, &entity_id, "", &jsonld_link_value()).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    assert!(body.get("speed").is_some(), "speed attribute should exist");
    pretty_assertions::assert_ne!(body["isParked"], old_is_parked, "isParked should have changed");

    delete_entity(&c, &entity_id).await;
    delete_registration(&c, &reg_id).await;
    mock.shutdown().await;
    scope.finish().await.expect("cleanup failed");
}

// ---------------------------------------------------------------------------
// Append entity attributes - exclusive mode
// ---------------------------------------------------------------------------

/// Append with exclusive registration: forwards exclusive attributes, does not store locally.
#[tokio::test]
async fn distop_entity_attrs_append_exclusive_forwards_not_local() {
    let scope = TestEnvironment::from_suite("distop_entity_attributes")
        .await
        .expect("isolated test environment");
    let c = client();
    let mock = MockServer::start().await;

    let entity_id = scope.id("Vehicle:append-exc");
    let reg_id = scope.id("Registration:append-exc");

    // Create entity locally
    let entity = vehicle_entity(&entity_id);
    let (status, _) = create_entity_local(&c, &entity).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    // Create exclusive registration for speed
    let reg = vehicle_registration_speed_redirection_ops(&reg_id, &entity_id, &mock.url("/broker1"), Some("exclusive"));
    let (status, _) = create_registration(&c, &reg).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let stub_path = format!("/broker1/ngsi-ld/v1/entities/{entity_id}/attrs");
    mock.set_stub(Method::POST, &stub_path, 204).await;

    // Append attributes
    let fragment = fragment_speed_isparked();
    let status = append_entity_attrs(&c, &entity_id, &fragment, &jsonld_link_value()).await;
    pretty_assertions::assert_eq!(status, StatusCode::NO_CONTENT);

    // Verify forwarding happened
    let count = mock.get_count(Method::POST, &stub_path);
    pretty_assertions::assert_eq!(count, 1, "forwarding should have happened");

    // Verify local entity does NOT contain speed (exclusive)
    let (status, _, body) = get_entity(&c, &entity_id, "local=true", &jsonld_link_value()).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    assert!(body.get("speed").is_none(), "speed should not be in local entity");

    delete_entity(&c, &entity_id).await;
    delete_registration(&c, &reg_id).await;
    mock.shutdown().await;
    scope.finish().await.expect("cleanup failed");
}

// ---------------------------------------------------------------------------
// Append entity attributes - redirect mode
// ---------------------------------------------------------------------------

/// Append with redirect registrations: forwards to multiple context sources, no local storage.
#[tokio::test]
async fn distop_entity_attrs_append_redirect_forwards_to_multiple() {
    let scope = TestEnvironment::from_suite("distop_entity_attributes")
        .await
        .expect("isolated test environment");
    let c = client();
    let mock = MockServer::start().await;

    let entity_id = scope.id("Vehicle:append-red");
    let reg_id1 = scope.id("Registration:append-red-1");
    let reg_id2 = scope.id("Registration:append-red-2");

    // Create two redirect registrations
    let reg1 = vehicle_registration_redirection_ops(&reg_id1, Some(&entity_id), &mock.url("/broker1"), Some("redirect"));
    let (status, _) = create_registration(&c, &reg1).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let reg2 = vehicle_registration_redirection_ops(&reg_id2, Some(&entity_id), &mock.url("/broker2"), Some("redirect"));
    let (status, _) = create_registration(&c, &reg2).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let stub_path1 = format!("/broker1/ngsi-ld/v1/entities/{entity_id}/attrs");
    let stub_path2 = format!("/broker2/ngsi-ld/v1/entities/{entity_id}/attrs");
    mock.set_stub(Method::POST, &stub_path1, 204).await;
    mock.set_stub(Method::POST, &stub_path2, 204).await;

    // Append attributes
    let fragment = fragment_speed_isparked();
    let status = append_entity_attrs(&c, &entity_id, &fragment, &jsonld_link_value()).await;
    pretty_assertions::assert_eq!(status, StatusCode::NO_CONTENT);

    // Verify both stubs called
    pretty_assertions::assert_eq!(mock.get_count(Method::POST, &stub_path1), 1);
    pretty_assertions::assert_eq!(mock.get_count(Method::POST, &stub_path2), 1);

    // Verify entity does not exist locally
    let (status, _, _) = get_entity(&c, &entity_id, "local=true", &jsonld_link_value()).await;
    pretty_assertions::assert_eq!(status, StatusCode::NOT_FOUND);

    delete_registration(&c, &reg_id1).await;
    delete_registration(&c, &reg_id2).await;
    mock.shutdown().await;
    scope.finish().await.expect("cleanup failed");
}

// ---------------------------------------------------------------------------
// Append entity attributes with noOverwrite
// ---------------------------------------------------------------------------

/// Append with noOverwrite and inclusive registration: existing attrs unchanged.
#[tokio::test]
async fn distop_entity_attrs_append_nooverwrite_inclusive_preserves_existing() {
    let scope = TestEnvironment::from_suite("distop_entity_attributes")
        .await
        .expect("isolated test environment");
    let c = client();
    let mock = MockServer::start().await;

    let entity_id = scope.id("Vehicle:append-noow-inc");
    let reg_id = scope.id("Registration:append-noow-inc");

    // Create entity
    let entity = vehicle_entity(&entity_id);
    let (status, _) = create_entity(&c, &entity).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    // Get original isParked value
    let (status, _, old_body) = get_entity(&c, &entity_id, "", &jsonld_link_value()).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    let old_is_parked = old_body["isParked"].clone();

    // Create inclusive registration
    let reg = vehicle_registration_redirection_ops(&reg_id, Some(&entity_id), &mock.url(""), Some("inclusive"));
    let (status, _) = create_registration(&c, &reg).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let stub_path = format!("/ngsi-ld/v1/entities/{entity_id}/attrs");
    mock.set_stub(Method::POST, &stub_path, 207).await;

    // Append attributes with noOverwrite
    let fragment = fragment_speed_isparked();
    let status = append_entity_attrs_no_overwrite(&c, &entity_id, &fragment, &jsonld_link_value()).await;
    pretty_assertions::assert_eq!(status, StatusCode::MULTI_STATUS);

    // Verify forwarding happened
    assert!(mock.get_count(Method::POST, &stub_path) > 0);

    // Verify speed exists but isParked unchanged
    let (status, _, body) = get_entity(&c, &entity_id, "", &jsonld_link_value()).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    assert!(body.get("speed").is_some(), "speed should exist");
    pretty_assertions::assert_eq!(body["isParked"], old_is_parked, "isParked should be unchanged");

    delete_entity(&c, &entity_id).await;
    delete_registration(&c, &reg_id).await;
    mock.shutdown().await;
    scope.finish().await.expect("cleanup failed");
}

/// Append with noOverwrite and exclusive registration.
#[tokio::test]
async fn distop_entity_attrs_append_nooverwrite_exclusive_forwards() {
    let scope = TestEnvironment::from_suite("distop_entity_attributes")
        .await
        .expect("isolated test environment");
    let c = client();
    let mock = MockServer::start().await;

    let entity_id = scope.id("Vehicle:append-noow-exc");
    let reg_id = scope.id("Registration:append-noow-exc");

    // Create entity locally
    let entity = vehicle_entity(&entity_id);
    let (status, _) = create_entity_local(&c, &entity).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    // Create exclusive registration
    let reg = vehicle_registration_speed_redirection_ops(&reg_id, &entity_id, &mock.url("/broker1"), Some("exclusive"));
    let (status, _) = create_registration(&c, &reg).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let stub_path = format!("/broker1/ngsi-ld/v1/entities/{entity_id}/attrs");
    mock.set_stub(Method::POST, &stub_path, 207).await;

    // Append attributes with noOverwrite
    let fragment = fragment_speed_isparked();
    let status = append_entity_attrs_no_overwrite(&c, &entity_id, &fragment, &jsonld_link_value()).await;
    pretty_assertions::assert_eq!(status, StatusCode::MULTI_STATUS);

    // Verify forwarding happened
    assert!(mock.get_count(Method::POST, &stub_path) > 0);

    delete_entity(&c, &entity_id).await;
    delete_registration(&c, &reg_id).await;
    mock.shutdown().await;
    scope.finish().await.expect("cleanup failed");
}

/// Append with noOverwrite and redirect registrations.
#[tokio::test]
async fn distop_entity_attrs_append_nooverwrite_redirect_forwards_to_multiple() {
    let scope = TestEnvironment::from_suite("distop_entity_attributes")
        .await
        .expect("isolated test environment");
    let c = client();
    let mock = MockServer::start().await;

    let entity_id = scope.id("Vehicle:append-noow-red");
    let reg_id1 = scope.id("Registration:append-noow-red-1");
    let reg_id2 = scope.id("Registration:append-noow-red-2");

    // Create two redirect registrations
    let reg1 = vehicle_registration_redirection_ops(&reg_id1, Some(&entity_id), &mock.url("/broker1"), Some("redirect"));
    let (status, _) = create_registration(&c, &reg1).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let reg2 = vehicle_registration_redirection_ops(&reg_id2, Some(&entity_id), &mock.url("/broker2"), Some("redirect"));
    let (status, _) = create_registration(&c, &reg2).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let stub_path1 = format!("/broker1/ngsi-ld/v1/entities/{entity_id}/attrs");
    let stub_path2 = format!("/broker2/ngsi-ld/v1/entities/{entity_id}/attrs");
    mock.set_stub(Method::POST, &stub_path1, 207).await;
    mock.set_stub(Method::POST, &stub_path2, 207).await;

    // Append attributes with noOverwrite
    let fragment = fragment_speed_isparked();
    let status = append_entity_attrs_no_overwrite(&c, &entity_id, &fragment, &jsonld_link_value()).await;
    pretty_assertions::assert_eq!(status, StatusCode::MULTI_STATUS);

    // Verify both stubs called
    assert!(mock.get_count(Method::POST, &stub_path1) > 0);
    assert!(mock.get_count(Method::POST, &stub_path2) > 0);

    delete_registration(&c, &reg_id1).await;
    delete_registration(&c, &reg_id2).await;
    mock.shutdown().await;
    scope.finish().await.expect("cleanup failed");
}

// ---------------------------------------------------------------------------
// Additional append tests
// ---------------------------------------------------------------------------

/// Append to non-existent entity returns 404.
#[tokio::test]
async fn distop_entity_attrs_append_not_found_returns_404() {
    let scope = TestEnvironment::from_suite("distop_entity_attributes")
        .await
        .expect("isolated test environment");
    let c = client();

    let entity_id = scope.id("Vehicle:append-notfound");
    let fragment = fragment_speed_isparked();
    let status = append_entity_attrs(&c, &entity_id, &fragment, &jsonld_link_value()).await;
    pretty_assertions::assert_eq!(status, StatusCode::NOT_FOUND);
    scope.finish().await.expect("cleanup failed");
}

/// Append with auxiliary registration only updates locally, no forwarding.
#[tokio::test]
async fn distop_entity_attrs_append_auxiliary_no_forwarding() {
    let scope = TestEnvironment::from_suite("distop_entity_attributes")
        .await
        .expect("isolated test environment");
    let c = client();
    let mock = MockServer::start().await;

    let entity_id = scope.id("Vehicle:append-aux");
    let reg_id = scope.id("Registration:append-aux");

    // Create entity locally
    let entity = vehicle_entity(&entity_id);
    let (status, _) = create_entity(&c, &entity).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    // Create auxiliary registration (retrieval only, no forwarding for provision)
    let reg = vehicle_registration_auxiliary_retrieve_ops(&reg_id, &entity_id, &mock.url(""));
    let (status, _) = create_registration(&c, &reg).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let stub_path = format!("/ngsi-ld/v1/entities/{entity_id}/attrs");
    mock.set_stub(Method::POST, &stub_path, 204).await;

    // Append attributes
    let fragment = fragment_speed_isparked();
    let status = append_entity_attrs(&c, &entity_id, &fragment, &jsonld_link_value()).await;
    pretty_assertions::assert_eq!(status, StatusCode::NO_CONTENT);

    // Verify no forwarding happened (auxiliary is retrieval-only)
    let count = mock.get_count(Method::POST, &stub_path);
    pretty_assertions::assert_eq!(count, 0, "auxiliary registration should not trigger forwarding");

    // Verify local entity was updated
    let (status, _, body) = get_entity(&c, &entity_id, "local=true", &jsonld_link_value()).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    assert!(body.get("speed").is_some(), "speed should exist locally");

    delete_entity(&c, &entity_id).await;
    delete_registration(&c, &reg_id).await;
    mock.shutdown().await;
    scope.finish().await.expect("cleanup failed");
}
