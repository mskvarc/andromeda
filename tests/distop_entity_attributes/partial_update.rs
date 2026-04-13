#![allow(clippy::expect_used, clippy::unwrap_used)]
//! Partial update entity attribute distributed operation tests.
//!
//! Tests for PATCH /entities/{entityId}/attrs/{attrId} with different registration modes.

use reqwest::{Method, StatusCode};
use serial_test::serial;

use crate::helpers::{
    MockServer,
    clean_database,
    client,
    create_entity,
    create_registration,
    delete_entity,
    delete_registration,
    fragment_speed_datasetid,
    get_entity,
    jsonld_link_value,
    partial_update_attr,
    unique_id,
    vehicle_entity_different,
    vehicle_registration_redirection_ops,
    vehicle_registration_speed_redirection_ops,
};

// ---------------------------------------------------------------------------
// Partial update attribute - inclusive mode
// ---------------------------------------------------------------------------

/// Partial update with inclusive registration: updates local and forwards.
#[tokio::test]
#[serial]
async fn distop_entity_attrs_partial_update_inclusive_forwards_and_updates_local() {
    clean_database().await;
    let c = client();
    let mut mock = MockServer::start().await;

    let entity_id = unique_id("urn:ngsi-ld:Vehicle:partial-inc");
    let reg_id = unique_id("urn:ngsi-ld:Registration:partial-inc");

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
    let count = mock.get_count(Method::PATCH, &stub_path).await;
    assert!(count > 0, "forwarding should have happened");

    // Verify local entity contains both isParked2 and speed
    let (status, _, body) = get_entity(&c, &entity_id, "local=true", &jsonld_link_value()).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    assert!(body.get("isParked2").is_some(), "isParked2 should exist");
    assert!(body.get("speed").is_some(), "speed should exist");

    delete_entity(&c, &entity_id).await;
    delete_registration(&c, &reg_id).await;
    mock.stop();
}

// ---------------------------------------------------------------------------
// Partial update attribute - exclusive mode
// ---------------------------------------------------------------------------

/// Partial update with exclusive registration: forwards but does not update local.
#[tokio::test]
#[serial]
async fn distop_entity_attrs_partial_update_exclusive_forwards_not_local() {
    clean_database().await;
    let c = client();
    let mut mock = MockServer::start().await;

    let entity_id = unique_id("urn:ngsi-ld:Vehicle:partial-exc");
    let reg_id = unique_id("urn:ngsi-ld:Registration:partial-exc");

    // Create entity with speed attribute
    let entity = vehicle_entity_different(&entity_id);
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
    let count = mock.get_count(Method::PATCH, &stub_path).await;
    assert!(count > 0, "forwarding should have happened");

    // Verify local entity does NOT contain speed (exclusive), isParked2 unchanged
    let (status, _, body) = get_entity(&c, &entity_id, "local=true", &jsonld_link_value()).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    assert!(body.get("speed").is_none(), "speed should not be in local entity");
    assert!(body.get("isParked2").is_some(), "isParked2 should be unchanged");

    delete_entity(&c, &entity_id).await;
    delete_registration(&c, &reg_id).await;
    mock.stop();
}

// ---------------------------------------------------------------------------
// Partial update attribute - redirect mode
// ---------------------------------------------------------------------------

/// Partial update with redirect registrations: forwards to multiple context sources.
#[tokio::test]
#[serial]
async fn distop_entity_attrs_partial_update_redirect_forwards_to_multiple() {
    clean_database().await;
    let c = client();
    let mut mock = MockServer::start().await;

    let entity_id = unique_id("urn:ngsi-ld:Vehicle:partial-red");
    let reg_id1 = unique_id("urn:ngsi-ld:Registration:partial-red-1");
    let reg_id2 = unique_id("urn:ngsi-ld:Registration:partial-red-2");

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
    assert!(mock.get_count(Method::PATCH, &stub_path1).await > 0);
    assert!(mock.get_count(Method::PATCH, &stub_path2).await > 0);

    delete_registration(&c, &reg_id1).await;
    delete_registration(&c, &reg_id2).await;
    mock.stop();
}

// ---------------------------------------------------------------------------
// Additional partial update tests
// ---------------------------------------------------------------------------

/// Partial update non-existent entity returns 404.
#[tokio::test]
#[serial]
async fn distop_entity_attrs_partial_update_not_found_returns_404() {
    clean_database().await;
    let c = client();

    let entity_id = unique_id("urn:ngsi-ld:Vehicle:partial-notfound");
    let fragment = fragment_speed_datasetid();
    let status = partial_update_attr(&c, &entity_id, "speed", &fragment, &jsonld_link_value()).await;
    pretty_assertions::assert_eq!(status, StatusCode::NOT_FOUND);
}

/// Partial update with auxiliary registration only updates locally, no forwarding.
#[tokio::test]
#[serial]
async fn distop_entity_attrs_partial_update_auxiliary_no_forwarding() {
    clean_database().await;
    let c = client();
    let mut mock = MockServer::start().await;

    let entity_id = unique_id("urn:ngsi-ld:Vehicle:partial-aux");
    let reg_id = unique_id("urn:ngsi-ld:Registration:partial-aux");

    // Create entity with speed attribute
    let entity = vehicle_entity_different(&entity_id);
    let (status, _) = create_entity(&c, &entity).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    // Create auxiliary registration (retrieval only)
    let reg = vehicle_registration_redirection_ops(&reg_id, Some(&entity_id), &mock.url(""), Some("auxiliary"));
    let (status, _) = create_registration(&c, &reg).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let stub_path = format!("/ngsi-ld/v1/entities/{entity_id}/attrs/speed");
    mock.set_stub(Method::PATCH, &stub_path, 204).await;

    // Partial update speed attribute
    let fragment = fragment_speed_datasetid();
    let status = partial_update_attr(&c, &entity_id, "speed", &fragment, &jsonld_link_value()).await;
    pretty_assertions::assert_eq!(status, StatusCode::NO_CONTENT);

    // Verify no forwarding happened
    let count = mock.get_count(Method::PATCH, &stub_path).await;
    pretty_assertions::assert_eq!(count, 0, "auxiliary registration should not trigger forwarding");

    delete_entity(&c, &entity_id).await;
    delete_registration(&c, &reg_id).await;
    mock.stop();
}
