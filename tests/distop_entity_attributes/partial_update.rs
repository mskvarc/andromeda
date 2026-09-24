#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]
//! Partial update entity attribute distributed operation tests.
//!
//! Tests for PATCH /entities/{entityId}/attrs/{attrId} with different registration modes.

use reqwest::{Method, StatusCode};

use crate::{
    context_source_adapter::ContextSourceAdapter as MockServer,
    helpers::{
        client,
        create_entity,
        create_registration,
        delete_entity,
        delete_registration,
        fragment_speed_datasetid,
        fragment_speed_no_datasetid,
        get_entity,
        jsonld_link_value,
        partial_update_attr,
        vehicle_entity,
        vehicle_entity_different,
        vehicle_registration_auxiliary_retrieve_ops,
        vehicle_registration_redirection_ops,
        vehicle_registration_speed_redirection_ops,
    },
};
use andromeda_test_harness::environment::TestEnvironment;

// ---------------------------------------------------------------------------
// Partial update attribute - inclusive mode
// ---------------------------------------------------------------------------

/// Partial update with inclusive registration: updates local and forwards.
#[tokio::test]
async fn distop_entity_attrs_partial_update_inclusive_forwards_and_updates_local() {
    let scope = TestEnvironment::from_suite("distop_entity_attributes")
        .await
        .expect("isolated test environment");
    let c = client();
    let mock = MockServer::start().await;

    let entity_id = scope.id("Vehicle:partial-inc");
    let reg_id = scope.id("Registration:partial-inc");

    // Create entity with speed attribute
    let entity = vehicle_entity_different(&entity_id);
    let (status, _) = create_entity(&c, &entity).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    // Create inclusive registration
    let reg = vehicle_registration_redirection_ops(&reg_id, Some(&entity_id), &mock.url(""), Some("inclusive"));
    let (status, _) = create_registration(&c, &reg).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let stub_path = format!("/ngsi-ld/v1/entities/{entity_id}/attrs/speed");
    mock.set_stub(Method::PATCH, &stub_path, 204).await;

    // Partial update speed attribute
    let fragment = fragment_speed_datasetid();
    let status = partial_update_attr(&c, &entity_id, "speed", &fragment, &jsonld_link_value()).await;
    pretty_assertions::assert_eq!(status, StatusCode::NO_CONTENT);

    // Verify forwarding happened
    let count = mock.get_count(Method::PATCH, &stub_path);
    assert!(count > 0, "forwarding should have happened");

    // Verify local entity contains both isParked2 and speed
    let (status, _, body) = get_entity(&c, &entity_id, "local=true", &jsonld_link_value()).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    assert!(body.get("isParked2").is_some(), "isParked2 should exist");
    assert!(body.get("speed").is_some(), "speed should exist");

    delete_entity(&c, &entity_id).await;
    delete_registration(&c, &reg_id).await;
    mock.shutdown().await;
    scope.finish().await.expect("cleanup failed");
}

// ---------------------------------------------------------------------------
// Partial update attribute - exclusive mode
// ---------------------------------------------------------------------------

/// Partial update with exclusive registration: forwards but does not update local.
#[tokio::test]
async fn distop_entity_attrs_partial_update_exclusive_forwards_not_local() {
    let scope = TestEnvironment::from_suite("distop_entity_attributes")
        .await
        .expect("isolated test environment");
    let c = client();
    let mock = MockServer::start().await;

    let entity_id = scope.id("Vehicle:partial-exc");
    let reg_id = scope.id("Registration:partial-exc");

    // Clause 5.9.2.4 refuses an exclusive registration with Conflict when the
    // existing Entity already holds one of the Attributes it covers, so the
    // local entity is created without speed.
    let entity = vehicle_entity(&entity_id);
    let (status, _) = create_entity(&c, &entity).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    // Create exclusive registration for speed
    let reg = vehicle_registration_speed_redirection_ops(&reg_id, &entity_id, &mock.url("/broker1"), Some("exclusive"));
    let (status, _) = create_registration(&c, &reg).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let stub_path = format!("/broker1/ngsi-ld/v1/entities/{entity_id}/attrs/speed");
    mock.set_stub(Method::PATCH, &stub_path, 204).await;

    // Partial update speed attribute
    let fragment = fragment_speed_datasetid();
    let status = partial_update_attr(&c, &entity_id, "speed", &fragment, &jsonld_link_value()).await;
    pretty_assertions::assert_eq!(status, StatusCode::NO_CONTENT);

    // Verify forwarding happened
    let count = mock.get_count(Method::PATCH, &stub_path);
    assert!(count > 0, "forwarding should have happened");

    // Verify local entity does NOT contain speed (exclusive), isParked unchanged
    let (status, _, body) = get_entity(&c, &entity_id, "local=true", &jsonld_link_value()).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    assert!(body.get("speed").is_none(), "speed should not be in local entity");
    assert!(body.get("isParked").is_some(), "isParked should be unchanged");

    delete_entity(&c, &entity_id).await;
    delete_registration(&c, &reg_id).await;
    mock.shutdown().await;
    scope.finish().await.expect("cleanup failed");
}

// ---------------------------------------------------------------------------
// Partial update attribute - redirect mode
// ---------------------------------------------------------------------------

/// Partial update with redirect registrations: forwards to multiple context sources.
#[tokio::test]
async fn distop_entity_attrs_partial_update_redirect_forwards_to_multiple() {
    let scope = TestEnvironment::from_suite("distop_entity_attributes")
        .await
        .expect("isolated test environment");
    let c = client();
    let mock = MockServer::start().await;

    let entity_id = scope.id("Vehicle:partial-red");
    let reg_id1 = scope.id("Registration:partial-red-1");
    let reg_id2 = scope.id("Registration:partial-red-2");

    // Create two redirect registrations
    let reg1 = vehicle_registration_redirection_ops(&reg_id1, Some(&entity_id), &mock.url("/broker1"), Some("redirect"));
    let (status, _) = create_registration(&c, &reg1).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let reg2 = vehicle_registration_redirection_ops(&reg_id2, Some(&entity_id), &mock.url("/broker2"), Some("redirect"));
    let (status, _) = create_registration(&c, &reg2).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let stub_path1 = format!("/broker1/ngsi-ld/v1/entities/{entity_id}/attrs/speed");
    let stub_path2 = format!("/broker2/ngsi-ld/v1/entities/{entity_id}/attrs/speed");
    mock.set_stub(Method::PATCH, &stub_path1, 204).await;
    mock.set_stub(Method::PATCH, &stub_path2, 204).await;

    // Partial update speed attribute
    let fragment = fragment_speed_datasetid();
    let status = partial_update_attr(&c, &entity_id, "speed", &fragment, &jsonld_link_value()).await;
    pretty_assertions::assert_eq!(status, StatusCode::NO_CONTENT);

    // Verify both stubs called
    assert!(mock.get_count(Method::PATCH, &stub_path1) > 0);
    assert!(mock.get_count(Method::PATCH, &stub_path2) > 0);

    delete_registration(&c, &reg_id1).await;
    delete_registration(&c, &reg_id2).await;
    mock.shutdown().await;
    scope.finish().await.expect("cleanup failed");
}

// ---------------------------------------------------------------------------
// Additional partial update tests
// ---------------------------------------------------------------------------

/// Partial update non-existent entity returns 404.
#[tokio::test]
async fn distop_entity_attrs_partial_update_not_found_returns_404() {
    let scope = TestEnvironment::from_suite("distop_entity_attributes")
        .await
        .expect("isolated test environment");
    let c = client();

    let entity_id = scope.id("Vehicle:partial-notfound");
    let fragment = fragment_speed_datasetid();
    let status = partial_update_attr(&c, &entity_id, "speed", &fragment, &jsonld_link_value()).await;
    pretty_assertions::assert_eq!(status, StatusCode::NOT_FOUND);
    scope.finish().await.expect("cleanup failed");
}

/// Partial update with auxiliary registration only updates locally, no forwarding.
#[tokio::test]
async fn distop_entity_attrs_partial_update_auxiliary_no_forwarding() {
    let scope = TestEnvironment::from_suite("distop_entity_attributes")
        .await
        .expect("isolated test environment");
    let c = client();
    let mock = MockServer::start().await;

    let entity_id = scope.id("Vehicle:partial-aux");
    let reg_id = scope.id("Registration:partial-aux");

    // Create entity with speed attribute
    let entity = vehicle_entity_different(&entity_id);
    let (status, _) = create_entity(&c, &entity).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    // Create auxiliary registration (retrieval only)
    let reg = vehicle_registration_auxiliary_retrieve_ops(&reg_id, &entity_id, &mock.url(""));
    let (status, _) = create_registration(&c, &reg).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let stub_path = format!("/ngsi-ld/v1/entities/{entity_id}/attrs/speed");
    mock.set_stub(Method::PATCH, &stub_path, 204).await;

    // Partial update the Entity's existing default-dataset speed. An auxiliary
    // registration does not support HTTP unsafe methods (clause 6.3.17), so the
    // update stays local and succeeds against the instance the Entity holds; a
    // fragment naming an absent datasetId would answer 404 here, with no forward
    // to mask the miss.
    let fragment = fragment_speed_no_datasetid();
    let status = partial_update_attr(&c, &entity_id, "speed", &fragment, &jsonld_link_value()).await;
    pretty_assertions::assert_eq!(status, StatusCode::NO_CONTENT);

    // Verify no forwarding happened
    let count = mock.get_count(Method::PATCH, &stub_path);
    pretty_assertions::assert_eq!(count, 0, "auxiliary registration should not trigger forwarding");

    delete_entity(&c, &entity_id).await;
    delete_registration(&c, &reg_id).await;
    mock.shutdown().await;
    scope.finish().await.expect("cleanup failed");
}
