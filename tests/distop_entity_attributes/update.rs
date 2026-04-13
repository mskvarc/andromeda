#![allow(clippy::expect_used, clippy::unwrap_used)]
//! Update entity attributes distributed operation tests.
//!
//! Tests for PATCH /entities/{entityId}/attrs with different registration modes.

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
    delete_registration,
    fragment_brandname,
    fragment_speed_different,
    get_entity,
    jsonld_link_value,
    unique_id,
    update_entity_attrs,
    vehicle_entity,
    vehicle_registration_redirection_ops,
    vehicle_registration_speed_redirection_ops,
};

// ---------------------------------------------------------------------------
// Update entity attributes - exclusive mode
// ---------------------------------------------------------------------------

/// Update with exclusive registration: forwards exclusive attributes to context source.
#[tokio::test]
#[serial]
async fn distop_entity_attrs_update_exclusive_forwards_to_context_source() {
    clean_database().await;
    let c = client();
    let mut mock = MockServer::start().await;

    let entity_id = unique_id("urn:ngsi-ld:Vehicle:update-exc");
    let reg_id = unique_id("urn:ngsi-ld:Registration:update-exc");

    // Create entity locally
    let entity = vehicle_entity(&entity_id);
    let (status, _) = create_entity_local(&c, &entity).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    // Create exclusive registration for speed
    let reg = vehicle_registration_speed_redirection_ops(&reg_id, &entity_id, &mock.url("/broker1"), Some("exclusive"));
    let (status, _) = create_registration(&c, &reg).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let stub_path = format!("/broker1/ngsi-ld/v1/entities/{entity_id}/attrs");
    mock.set_stub(Method::PATCH, &stub_path, 204).await;

    // Update entity attributes
    let fragment = fragment_speed_different();
    let status = update_entity_attrs(&c, &entity_id, &fragment, &jsonld_link_value()).await;
    pretty_assertions::assert_eq!(status, StatusCode::NO_CONTENT);

    // Verify forwarding happened
    let count = mock.get_count(Method::PATCH, &stub_path).await;
    assert!(count > 0, "forwarding should have happened");

    delete_entity(&c, &entity_id).await;
    delete_registration(&c, &reg_id).await;
    mock.stop();
}

// ---------------------------------------------------------------------------
// Update entity attributes - inclusive mode
// ---------------------------------------------------------------------------

/// Update with inclusive registration: local fails (no matching attr), remote succeeds -> 207.
#[tokio::test]
#[serial]
async fn distop_entity_attrs_update_inclusive_partial_success_returns_207() {
    clean_database().await;
    let c = client();
    let mut mock = MockServer::start().await;

    let entity_id = unique_id("urn:ngsi-ld:Vehicle:update-inc");
    let reg_id = unique_id("urn:ngsi-ld:Registration:update-inc");

    // No local entity -- only registration
    let reg = vehicle_registration_redirection_ops(&reg_id, Some(&entity_id), &mock.url(""), None);
    let (status, _) = create_registration(&c, &reg).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let stub_path = format!("/ngsi-ld/v1/entities/{entity_id}/attrs");
    mock.set_stub(Method::PATCH, &stub_path, 204).await;

    // Update entity attributes -- local fails (no entity), remote succeeds -> 207
    let fragment = fragment_brandname();
    let status = update_entity_attrs(&c, &entity_id, &fragment, &jsonld_link_value()).await;
    pretty_assertions::assert_eq!(status, StatusCode::MULTI_STATUS);

    // Verify forwarding happened
    let count = mock.get_count(Method::PATCH, &stub_path).await;
    assert!(count > 0, "forwarding should have happened");

    delete_registration(&c, &reg_id).await;
    mock.stop();
}

/// Update with inclusive registration: both local and remote succeed.
#[tokio::test]
#[serial]
async fn distop_entity_attrs_update_inclusive_both_succeed() {
    clean_database().await;
    let c = client();
    let mut mock = MockServer::start().await;

    let entity_id = unique_id("urn:ngsi-ld:Vehicle:update-inc-both");
    let reg_id = unique_id("urn:ngsi-ld:Registration:update-inc-both");

    // Create entity locally
    let entity = vehicle_entity(&entity_id);
    let (status, _) = create_entity(&c, &entity).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    // Create inclusive registration
    let reg = vehicle_registration_redirection_ops(&reg_id, Some(&entity_id), &mock.url(""), Some("inclusive"));
    let (status, _) = create_registration(&c, &reg).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let stub_path = format!("/ngsi-ld/v1/entities/{entity_id}/attrs");
    mock.set_stub(Method::PATCH, &stub_path, 204).await;

    // Update brandName (exists locally)
    let fragment = fragment_brandname();
    let status = update_entity_attrs(&c, &entity_id, &fragment, &jsonld_link_value()).await;
    pretty_assertions::assert_eq!(status, StatusCode::NO_CONTENT);

    // Verify forwarding happened
    let count = mock.get_count(Method::PATCH, &stub_path).await;
    assert!(count > 0, "forwarding should have happened");

    // Verify local entity was updated
    let (status, _, body) = get_entity(&c, &entity_id, "", &jsonld_link_value()).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    pretty_assertions::assert_eq!(body["brandName"]["value"], "BMW");

    delete_entity(&c, &entity_id).await;
    delete_registration(&c, &reg_id).await;
    mock.stop();
}

// ---------------------------------------------------------------------------
// Update entity attributes - redirect mode
// ---------------------------------------------------------------------------

/// Update with redirect registrations: forwards to multiple context sources.
#[tokio::test]
#[serial]
async fn distop_entity_attrs_update_redirect_forwards_to_multiple() {
    clean_database().await;
    let c = client();
    let mut mock = MockServer::start().await;

    let entity_id = unique_id("urn:ngsi-ld:Vehicle:update-red");
    let reg_id1 = unique_id("urn:ngsi-ld:Registration:update-red-1");
    let reg_id2 = unique_id("urn:ngsi-ld:Registration:update-red-2");

    // Create two redirect registrations
    let reg1 = vehicle_registration_redirection_ops(&reg_id1, Some(&entity_id), &mock.url("/broker1"), Some("redirect"));
    let (status, _) = create_registration(&c, &reg1).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let reg2 = vehicle_registration_redirection_ops(&reg_id2, Some(&entity_id), &mock.url("/broker2"), Some("redirect"));
    let (status, _) = create_registration(&c, &reg2).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let stub_path1 = format!("/broker1/ngsi-ld/v1/entities/{entity_id}/attrs");
    let stub_path2 = format!("/broker2/ngsi-ld/v1/entities/{entity_id}/attrs");
    mock.set_stub(Method::PATCH, &stub_path1, 204).await;
    mock.set_stub(Method::PATCH, &stub_path2, 204).await;

    // Update entity attributes
    let fragment = fragment_brandname();
    let status = update_entity_attrs(&c, &entity_id, &fragment, &jsonld_link_value()).await;
    pretty_assertions::assert_eq!(status, StatusCode::NO_CONTENT);

    // Verify both stubs called
    assert!(mock.get_count(Method::PATCH, &stub_path1).await > 0);
    assert!(mock.get_count(Method::PATCH, &stub_path2).await > 0);

    delete_registration(&c, &reg_id1).await;
    delete_registration(&c, &reg_id2).await;
    mock.stop();
}

// ---------------------------------------------------------------------------
// Additional update tests
// ---------------------------------------------------------------------------

/// Update non-existent entity returns 404.
#[tokio::test]
#[serial]
async fn distop_entity_attrs_update_not_found_returns_404() {
    clean_database().await;
    let c = client();

    let entity_id = unique_id("urn:ngsi-ld:Vehicle:update-notfound");
    let fragment = fragment_brandname();
    let status = update_entity_attrs(&c, &entity_id, &fragment, &jsonld_link_value()).await;
    pretty_assertions::assert_eq!(status, StatusCode::NOT_FOUND);
}

/// Update with auxiliary registration only updates locally, no forwarding.
#[tokio::test]
#[serial]
async fn distop_entity_attrs_update_auxiliary_no_forwarding() {
    clean_database().await;
    let c = client();
    let mut mock = MockServer::start().await;

    let entity_id = unique_id("urn:ngsi-ld:Vehicle:update-aux");
    let reg_id = unique_id("urn:ngsi-ld:Registration:update-aux");

    // Create entity locally
    let entity = vehicle_entity(&entity_id);
    let (status, _) = create_entity(&c, &entity).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    // Create auxiliary registration (retrieval only)
    let reg = vehicle_registration_redirection_ops(&reg_id, Some(&entity_id), &mock.url(""), Some("auxiliary"));
    let (status, _) = create_registration(&c, &reg).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let stub_path = format!("/ngsi-ld/v1/entities/{entity_id}/attrs");
    mock.set_stub(Method::PATCH, &stub_path, 204).await;

    // Update entity attributes
    let fragment = fragment_brandname();
    let status = update_entity_attrs(&c, &entity_id, &fragment, &jsonld_link_value()).await;
    pretty_assertions::assert_eq!(status, StatusCode::NO_CONTENT);

    // Verify no forwarding happened
    let count = mock.get_count(Method::PATCH, &stub_path).await;
    pretty_assertions::assert_eq!(count, 0, "auxiliary registration should not trigger forwarding");

    // Verify local entity was updated
    let (status, _, body) = get_entity(&c, &entity_id, "local=true", &jsonld_link_value()).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    pretty_assertions::assert_eq!(body["brandName"]["value"], "BMW");

    delete_entity(&c, &entity_id).await;
    delete_registration(&c, &reg_id).await;
    mock.stop();
}
