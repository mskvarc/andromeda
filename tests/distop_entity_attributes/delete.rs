#![allow(clippy::expect_used, clippy::unwrap_used)]
//! Delete entity attribute distributed operation tests.
//!
//! Tests for DELETE /entities/{entityId}/attrs/{attrId} with different registration modes.

use reqwest::{Method, StatusCode};
use serial_test::serial;

use crate::helpers::{
    MockServer,
    clean_database,
    client,
    create_entity,
    create_entity_local,
    create_registration,
    delete_entity,
    delete_entity_attr,
    delete_entity_attr_all,
    delete_registration,
    get_entity,
    jsonld_link_value,
    unique_id,
    vehicle_entity,
    vehicle_entity_two_speed_datasets,
    vehicle_registration_redirection_ops,
    vehicle_registration_speed_redirection_ops,
};

// ---------------------------------------------------------------------------
// Delete entity attribute (default instance) - inclusive mode
// ---------------------------------------------------------------------------

/// Delete attribute (default instance) with inclusive registration.
#[tokio::test]
#[serial]
async fn distop_entity_attrs_delete_inclusive_forwards_and_deletes_local() {
    clean_database().await;
    let c = client();
    let mut mock = MockServer::start().await;

    let entity_id = unique_id("urn:ngsi-ld:Vehicle:delete-inc");
    let reg_id = unique_id("urn:ngsi-ld:Registration:delete-inc");

    // Create entity with two datasetId speed attributes
    let entity = vehicle_entity_two_speed_datasets(&entity_id);
    let (status, _) = create_entity(&c, &entity).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    // Create inclusive registration
    let reg = vehicle_registration_redirection_ops(&reg_id, Some(&entity_id), &mock.url(""), Some("inclusive"));
    let (status, _) = create_registration(&c, &reg).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let stub_path = format!("/ngsi-ld/v1/entities/{entity_id}/attrs/speed");
    mock.set_stub(Method::DELETE, &stub_path, 204).await;

    // Delete attribute (default instance)
    let status = delete_entity_attr(&c, &entity_id, "speed").await;
    pretty_assertions::assert_eq!(status, StatusCode::NO_CONTENT);

    // Verify forwarding happened
    let count = mock.get_count(Method::DELETE, &stub_path).await;
    assert!(count > 0, "forwarding should have happened");

    // Verify local entity still contains speed (only default instance deleted)
    let (status, _, body) = get_entity(&c, &entity_id, "local=true", &jsonld_link_value()).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    assert!(body.get("speed").is_some(), "speed should still exist (other instances remain)");

    delete_entity(&c, &entity_id).await;
    delete_registration(&c, &reg_id).await;
    mock.stop();
}

// ---------------------------------------------------------------------------
// Delete entity attribute (default instance) - exclusive mode
// ---------------------------------------------------------------------------

/// Delete attribute (default instance) with exclusive registration.
#[tokio::test]
#[serial]
async fn distop_entity_attrs_delete_exclusive_forwards_to_context_source() {
    clean_database().await;
    let c = client();
    let mut mock = MockServer::start().await;

    let entity_id = unique_id("urn:ngsi-ld:Vehicle:delete-exc");
    let reg_id = unique_id("urn:ngsi-ld:Registration:delete-exc");

    // Create entity locally
    let entity = vehicle_entity(&entity_id);
    let (status, _) = create_entity_local(&c, &entity).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    // Create exclusive registration for speed
    let reg = vehicle_registration_speed_redirection_ops(&reg_id, &entity_id, &mock.url("/broker1"), Some("exclusive"));
    let (status, _) = create_registration(&c, &reg).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let stub_path = format!("/broker1/ngsi-ld/v1/entities/{entity_id}/attrs/speed");
    mock.set_stub(Method::DELETE, &stub_path, 204).await;

    // Delete attribute
    let status = delete_entity_attr(&c, &entity_id, "speed").await;
    pretty_assertions::assert_eq!(status, StatusCode::NO_CONTENT);

    // Verify forwarding happened
    let count = mock.get_count(Method::DELETE, &stub_path).await;
    assert!(count > 0, "forwarding should have happened");

    delete_entity(&c, &entity_id).await;
    delete_registration(&c, &reg_id).await;
    mock.stop();
}

// ---------------------------------------------------------------------------
// Delete entity attribute (default instance) - redirect mode
// ---------------------------------------------------------------------------

/// Delete attribute (default instance) with redirect registrations.
#[tokio::test]
#[serial]
async fn distop_entity_attrs_delete_redirect_forwards_to_multiple() {
    clean_database().await;
    let c = client();
    let mut mock = MockServer::start().await;

    let entity_id = unique_id("urn:ngsi-ld:Vehicle:delete-red");
    let reg_id1 = unique_id("urn:ngsi-ld:Registration:delete-red-1");
    let reg_id2 = unique_id("urn:ngsi-ld:Registration:delete-red-2");

    // Create entity
    let entity = vehicle_entity_two_speed_datasets(&entity_id);
    let (status, _) = create_entity(&c, &entity).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    // Create two redirect registrations
    let reg1 = vehicle_registration_redirection_ops(&reg_id1, Some(&entity_id), &mock.url("/broker1"), Some("redirect"));
    let (status, _) = create_registration(&c, &reg1).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let reg2 = vehicle_registration_redirection_ops(&reg_id2, Some(&entity_id), &mock.url("/broker2"), Some("redirect"));
    let (status, _) = create_registration(&c, &reg2).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let stub_path1 = format!("/broker1/ngsi-ld/v1/entities/{entity_id}/attrs/speed");
    let stub_path2 = format!("/broker2/ngsi-ld/v1/entities/{entity_id}/attrs/speed");
    mock.set_stub(Method::DELETE, &stub_path1, 204).await;
    mock.set_stub(Method::DELETE, &stub_path2, 204).await;

    // Delete attribute
    let status = delete_entity_attr(&c, &entity_id, "speed").await;
    pretty_assertions::assert_eq!(status, StatusCode::NO_CONTENT);

    // Verify both stubs called
    assert!(mock.get_count(Method::DELETE, &stub_path1).await > 0);
    assert!(mock.get_count(Method::DELETE, &stub_path2).await > 0);

    delete_entity(&c, &entity_id).await;
    delete_registration(&c, &reg_id1).await;
    delete_registration(&c, &reg_id2).await;
    mock.stop();
}

// ---------------------------------------------------------------------------
// Delete entity attribute (deleteAll=true) - inclusive mode
// ---------------------------------------------------------------------------

/// Delete attribute with deleteAll=true and inclusive registration.
#[tokio::test]
#[serial]
async fn distop_entity_attrs_delete_all_inclusive_deletes_all_instances() {
    clean_database().await;
    let c = client();
    let mut mock = MockServer::start().await;

    let entity_id = unique_id("urn:ngsi-ld:Vehicle:delete-all-inc");
    let reg_id = unique_id("urn:ngsi-ld:Registration:delete-all-inc");

    // Create entity with two datasetId speed attributes
    let entity = vehicle_entity_two_speed_datasets(&entity_id);
    let (status, _) = create_entity(&c, &entity).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    // Create inclusive registration
    let reg = vehicle_registration_redirection_ops(&reg_id, Some(&entity_id), &mock.url(""), Some("inclusive"));
    let (status, _) = create_registration(&c, &reg).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let stub_path = format!("/ngsi-ld/v1/entities/{entity_id}/attrs/speed");
    mock.set_stub(Method::DELETE, &stub_path, 204).await;

    // Delete attribute with deleteAll=true
    let status = delete_entity_attr_all(&c, &entity_id, "speed").await;
    pretty_assertions::assert_eq!(status, StatusCode::NO_CONTENT);

    // Verify forwarding happened
    let count = mock.get_count(Method::DELETE, &stub_path).await;
    assert!(count > 0, "forwarding should have happened");

    // Verify local entity does NOT contain speed (all instances deleted)
    let (status, _, body) = get_entity(&c, &entity_id, "local=true", &jsonld_link_value()).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    assert!(body.get("speed").is_none(), "speed should be fully deleted");

    delete_entity(&c, &entity_id).await;
    delete_registration(&c, &reg_id).await;
    mock.stop();
}

// ---------------------------------------------------------------------------
// Delete entity attribute (deleteAll=true) - exclusive mode
// ---------------------------------------------------------------------------

/// Delete attribute with deleteAll=true and exclusive registration.
#[tokio::test]
#[serial]
async fn distop_entity_attrs_delete_all_exclusive_forwards_to_context_source() {
    clean_database().await;
    let c = client();
    let mut mock = MockServer::start().await;

    let entity_id = unique_id("urn:ngsi-ld:Vehicle:delete-all-exc");
    let reg_id = unique_id("urn:ngsi-ld:Registration:delete-all-exc");

    // Create entity locally
    let entity = vehicle_entity(&entity_id);
    let (status, _) = create_entity_local(&c, &entity).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    // Create exclusive registration for speed
    let reg = vehicle_registration_speed_redirection_ops(&reg_id, &entity_id, &mock.url("/broker1"), Some("exclusive"));
    let (status, _) = create_registration(&c, &reg).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let stub_path = format!("/broker1/ngsi-ld/v1/entities/{entity_id}/attrs/speed");
    mock.set_stub(Method::DELETE, &stub_path, 204).await;

    // Delete attribute with deleteAll=true
    let status = delete_entity_attr_all(&c, &entity_id, "speed").await;
    pretty_assertions::assert_eq!(status, StatusCode::NO_CONTENT);

    // Verify forwarding happened
    let count = mock.get_count(Method::DELETE, &stub_path).await;
    assert!(count > 0, "forwarding should have happened");

    delete_entity(&c, &entity_id).await;
    delete_registration(&c, &reg_id).await;
    mock.stop();
}

// ---------------------------------------------------------------------------
// Delete entity attribute (deleteAll=true) - redirect mode
// ---------------------------------------------------------------------------

/// Delete attribute with deleteAll=true and redirect registrations.
#[tokio::test]
#[serial]
async fn distop_entity_attrs_delete_all_redirect_forwards_to_multiple() {
    clean_database().await;
    let c = client();
    let mut mock = MockServer::start().await;

    let entity_id = unique_id("urn:ngsi-ld:Vehicle:delete-all-red");
    let reg_id1 = unique_id("urn:ngsi-ld:Registration:delete-all-red-1");
    let reg_id2 = unique_id("urn:ngsi-ld:Registration:delete-all-red-2");

    // Create entity
    let entity = vehicle_entity_two_speed_datasets(&entity_id);
    let (status, _) = create_entity(&c, &entity).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    // Create two redirect registrations
    let reg1 = vehicle_registration_redirection_ops(&reg_id1, Some(&entity_id), &mock.url("/broker1"), Some("redirect"));
    let (status, _) = create_registration(&c, &reg1).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let reg2 = vehicle_registration_redirection_ops(&reg_id2, Some(&entity_id), &mock.url("/broker2"), Some("redirect"));
    let (status, _) = create_registration(&c, &reg2).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let stub_path1 = format!("/broker1/ngsi-ld/v1/entities/{entity_id}/attrs/speed");
    let stub_path2 = format!("/broker2/ngsi-ld/v1/entities/{entity_id}/attrs/speed");
    mock.set_stub(Method::DELETE, &stub_path1, 204).await;
    mock.set_stub(Method::DELETE, &stub_path2, 204).await;

    // Delete attribute with deleteAll=true
    let status = delete_entity_attr_all(&c, &entity_id, "speed").await;
    pretty_assertions::assert_eq!(status, StatusCode::NO_CONTENT);

    // Verify both stubs called
    assert!(mock.get_count(Method::DELETE, &stub_path1).await > 0);
    assert!(mock.get_count(Method::DELETE, &stub_path2).await > 0);

    delete_entity(&c, &entity_id).await;
    delete_registration(&c, &reg_id1).await;
    delete_registration(&c, &reg_id2).await;
    mock.stop();
}

// ---------------------------------------------------------------------------
// Additional delete tests
// ---------------------------------------------------------------------------

/// Delete attribute from non-existent entity returns 404.
#[tokio::test]
#[serial]
async fn distop_entity_attrs_delete_not_found_returns_404() {
    clean_database().await;
    let c = client();

    let entity_id = unique_id("urn:ngsi-ld:Vehicle:delete-notfound");
    let status = delete_entity_attr(&c, &entity_id, "speed").await;
    pretty_assertions::assert_eq!(status, StatusCode::NOT_FOUND);
}

/// Delete with auxiliary registration only deletes locally, no forwarding.
#[tokio::test]
#[serial]
async fn distop_entity_attrs_delete_auxiliary_no_forwarding() {
    clean_database().await;
    let c = client();
    let mut mock = MockServer::start().await;

    let entity_id = unique_id("urn:ngsi-ld:Vehicle:delete-aux");
    let reg_id = unique_id("urn:ngsi-ld:Registration:delete-aux");

    // Create entity with two datasetId speed attributes
    let entity = vehicle_entity_two_speed_datasets(&entity_id);
    let (status, _) = create_entity(&c, &entity).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    // Create auxiliary registration (retrieval only)
    let reg = vehicle_registration_redirection_ops(&reg_id, Some(&entity_id), &mock.url(""), Some("auxiliary"));
    let (status, _) = create_registration(&c, &reg).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let stub_path = format!("/ngsi-ld/v1/entities/{entity_id}/attrs/speed");
    mock.set_stub(Method::DELETE, &stub_path, 204).await;

    // Delete attribute
    let status = delete_entity_attr(&c, &entity_id, "speed").await;
    pretty_assertions::assert_eq!(status, StatusCode::NO_CONTENT);

    // Verify no forwarding happened
    let count = mock.get_count(Method::DELETE, &stub_path).await;
    pretty_assertions::assert_eq!(count, 0, "auxiliary registration should not trigger forwarding");

    delete_entity(&c, &entity_id).await;
    delete_registration(&c, &reg_id).await;
    mock.stop();
}
