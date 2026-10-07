#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]
//! Delete entity attribute distributed operation tests.
//!
//! Tests for DELETE /entities/{entityId}/attrs/{attrId} with different registration modes.

use reqwest::{Method, StatusCode};

use crate::{
    context_source_adapter::ContextSourceAdapter as MockServer,
    helpers::{
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
        vehicle_entity,
        vehicle_entity_two_speed_datasets,
        vehicle_registration_auxiliary_retrieve_ops,
        vehicle_registration_redirection_ops,
        vehicle_registration_speed_redirection_ops,
    },
};
use andromeda_test_harness::environment::TestEnvironment;

/// Delete attribute (default instance) with inclusive registration.
#[tokio::test]
async fn distop_entity_attrs_delete_inclusive_forwards_and_deletes_local() {
    let scope = TestEnvironment::from_suite("distop_entity_attributes")
        .await
        .expect("isolated test environment");
    let c = client();
    let mock = MockServer::start().await;

    let entity_id = scope.id("Vehicle:delete-inc");
    let reg_id = scope.id("Registration:delete-inc");

    let entity = vehicle_entity_two_speed_datasets(&entity_id);
    let (status, _) = create_entity(&c, &entity).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let reg = vehicle_registration_redirection_ops(&reg_id, Some(&entity_id), &mock.url(""), Some("inclusive"));
    let (status, _) = create_registration(&c, &reg).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let stub_path = format!("/ngsi-ld/v1/entities/{entity_id}/attrs/speed");
    mock.set_stub(Method::DELETE, &stub_path, 204).await;

    let status = delete_entity_attr(&c, &entity_id, "speed", &jsonld_link_value()).await;
    pretty_assertions::assert_eq!(status, StatusCode::NO_CONTENT);

    let count = mock.get_count(Method::DELETE, &stub_path);
    assert!(count > 0, "forwarding should have happened");

    let (status, _, body) = get_entity(&c, &entity_id, "local=true", &jsonld_link_value()).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    assert!(body.get("speed").is_some(), "speed should still exist (other instances remain)");

    delete_entity(&c, &entity_id).await;
    delete_registration(&c, &reg_id).await;
    mock.shutdown().await;
    scope.finish().await.expect("cleanup failed");
}

/// Delete attribute (default instance) with exclusive registration.
#[tokio::test]
async fn distop_entity_attrs_delete_exclusive_forwards_to_context_source() {
    let scope = TestEnvironment::from_suite("distop_entity_attributes")
        .await
        .expect("isolated test environment");
    let c = client();
    let mock = MockServer::start().await;

    let entity_id = scope.id("Vehicle:delete-exc");
    let reg_id = scope.id("Registration:delete-exc");

    let entity = vehicle_entity(&entity_id);
    let (status, _) = create_entity_local(&c, &entity).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let reg = vehicle_registration_speed_redirection_ops(&reg_id, &entity_id, &mock.url("/broker1"), Some("exclusive"));
    let (status, _) = create_registration(&c, &reg).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let stub_path = format!("/broker1/ngsi-ld/v1/entities/{entity_id}/attrs/speed");
    mock.set_stub(Method::DELETE, &stub_path, 204).await;

    let status = delete_entity_attr(&c, &entity_id, "speed", &jsonld_link_value()).await;
    pretty_assertions::assert_eq!(status, StatusCode::NO_CONTENT);

    let count = mock.get_count(Method::DELETE, &stub_path);
    assert!(count > 0, "forwarding should have happened");

    delete_entity(&c, &entity_id).await;
    delete_registration(&c, &reg_id).await;
    mock.shutdown().await;
    scope.finish().await.expect("cleanup failed");
}

/// Delete attribute (default instance) with redirect registrations.
#[tokio::test]
async fn distop_entity_attrs_delete_redirect_forwards_to_multiple() {
    let scope = TestEnvironment::from_suite("distop_entity_attributes")
        .await
        .expect("isolated test environment");
    let c = client();
    let mock = MockServer::start().await;

    let entity_id = scope.id("Vehicle:delete-red");
    let reg_id1 = scope.id("Registration:delete-red-1");
    let reg_id2 = scope.id("Registration:delete-red-2");

    // Two redirect registrations. Clause 5.9.2.4 refuses a redirect registration
    // with Conflict when an existing Entity already matches it, so the entity is
    // held only by the registered context sources.
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

    let status = delete_entity_attr(&c, &entity_id, "speed", &jsonld_link_value()).await;
    pretty_assertions::assert_eq!(status, StatusCode::NO_CONTENT);

    assert!(mock.get_count(Method::DELETE, &stub_path1) > 0);
    assert!(mock.get_count(Method::DELETE, &stub_path2) > 0);

    delete_registration(&c, &reg_id1).await;
    delete_registration(&c, &reg_id2).await;
    mock.shutdown().await;
    scope.finish().await.expect("cleanup failed");
}

/// Delete attribute with deleteAll=true and inclusive registration.
#[tokio::test]
async fn distop_entity_attrs_delete_all_inclusive_deletes_all_instances() {
    let scope = TestEnvironment::from_suite("distop_entity_attributes")
        .await
        .expect("isolated test environment");
    let c = client();
    let mock = MockServer::start().await;

    let entity_id = scope.id("Vehicle:delete-all-inc");
    let reg_id = scope.id("Registration:delete-all-inc");

    let entity = vehicle_entity_two_speed_datasets(&entity_id);
    let (status, _) = create_entity(&c, &entity).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let reg = vehicle_registration_redirection_ops(&reg_id, Some(&entity_id), &mock.url(""), Some("inclusive"));
    let (status, _) = create_registration(&c, &reg).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let stub_path = format!("/ngsi-ld/v1/entities/{entity_id}/attrs/speed");
    mock.set_stub(Method::DELETE, &stub_path, 204).await;

    let status = delete_entity_attr_all(&c, &entity_id, "speed", &jsonld_link_value()).await;
    pretty_assertions::assert_eq!(status, StatusCode::NO_CONTENT);

    let count = mock.get_count(Method::DELETE, &stub_path);
    assert!(count > 0, "forwarding should have happened");

    let (status, _, body) = get_entity(&c, &entity_id, "local=true", &jsonld_link_value()).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    assert!(body.get("speed").is_none(), "speed should be fully deleted");

    delete_entity(&c, &entity_id).await;
    delete_registration(&c, &reg_id).await;
    mock.shutdown().await;
    scope.finish().await.expect("cleanup failed");
}

/// Delete attribute with deleteAll=true and exclusive registration.
#[tokio::test]
async fn distop_entity_attrs_delete_all_exclusive_forwards_to_context_source() {
    let scope = TestEnvironment::from_suite("distop_entity_attributes")
        .await
        .expect("isolated test environment");
    let c = client();
    let mock = MockServer::start().await;

    let entity_id = scope.id("Vehicle:delete-all-exc");
    let reg_id = scope.id("Registration:delete-all-exc");

    let entity = vehicle_entity(&entity_id);
    let (status, _) = create_entity_local(&c, &entity).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let reg = vehicle_registration_speed_redirection_ops(&reg_id, &entity_id, &mock.url("/broker1"), Some("exclusive"));
    let (status, _) = create_registration(&c, &reg).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let stub_path = format!("/broker1/ngsi-ld/v1/entities/{entity_id}/attrs/speed");
    mock.set_stub(Method::DELETE, &stub_path, 204).await;

    let status = delete_entity_attr_all(&c, &entity_id, "speed", &jsonld_link_value()).await;
    pretty_assertions::assert_eq!(status, StatusCode::NO_CONTENT);

    let count = mock.get_count(Method::DELETE, &stub_path);
    assert!(count > 0, "forwarding should have happened");

    delete_entity(&c, &entity_id).await;
    delete_registration(&c, &reg_id).await;
    mock.shutdown().await;
    scope.finish().await.expect("cleanup failed");
}

/// Delete attribute with deleteAll=true and redirect registrations.
#[tokio::test]
async fn distop_entity_attrs_delete_all_redirect_forwards_to_multiple() {
    let scope = TestEnvironment::from_suite("distop_entity_attributes")
        .await
        .expect("isolated test environment");
    let c = client();
    let mock = MockServer::start().await;

    let entity_id = scope.id("Vehicle:delete-all-red");
    let reg_id1 = scope.id("Registration:delete-all-red-1");
    let reg_id2 = scope.id("Registration:delete-all-red-2");

    // Two redirect registrations. Clause 5.9.2.4 refuses a redirect registration
    // with Conflict when an existing Entity already matches it, so the entity is
    // held only by the registered context sources.
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

    let status = delete_entity_attr_all(&c, &entity_id, "speed", &jsonld_link_value()).await;
    pretty_assertions::assert_eq!(status, StatusCode::NO_CONTENT);

    assert!(mock.get_count(Method::DELETE, &stub_path1) > 0);
    assert!(mock.get_count(Method::DELETE, &stub_path2) > 0);

    delete_registration(&c, &reg_id1).await;
    delete_registration(&c, &reg_id2).await;
    mock.shutdown().await;
    scope.finish().await.expect("cleanup failed");
}

/// Delete attribute from non-existent entity returns 404.
#[tokio::test]
async fn distop_entity_attrs_delete_not_found_returns_404() {
    let scope = TestEnvironment::from_suite("distop_entity_attributes")
        .await
        .expect("isolated test environment");
    let c = client();

    let entity_id = scope.id("Vehicle:delete-notfound");
    let status = delete_entity_attr(&c, &entity_id, "speed", &jsonld_link_value()).await;
    pretty_assertions::assert_eq!(status, StatusCode::NOT_FOUND);
    scope.finish().await.expect("cleanup failed");
}

/// Delete with auxiliary registration only deletes locally, no forwarding.
#[tokio::test]
async fn distop_entity_attrs_delete_auxiliary_no_forwarding() {
    let scope = TestEnvironment::from_suite("distop_entity_attributes")
        .await
        .expect("isolated test environment");
    let c = client();
    let mock = MockServer::start().await;

    let entity_id = scope.id("Vehicle:delete-aux");
    let reg_id = scope.id("Registration:delete-aux");

    let entity = vehicle_entity_two_speed_datasets(&entity_id);
    let (status, _) = create_entity(&c, &entity).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let reg = vehicle_registration_auxiliary_retrieve_ops(&reg_id, &entity_id, &mock.url(""));
    let (status, _) = create_registration(&c, &reg).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let stub_path = format!("/ngsi-ld/v1/entities/{entity_id}/attrs/speed");
    mock.set_stub(Method::DELETE, &stub_path, 204).await;

    let status = delete_entity_attr(&c, &entity_id, "speed", &jsonld_link_value()).await;
    pretty_assertions::assert_eq!(status, StatusCode::NO_CONTENT);

    let count = mock.get_count(Method::DELETE, &stub_path);
    pretty_assertions::assert_eq!(count, 0, "auxiliary registration should not trigger forwarding");

    delete_entity(&c, &entity_id).await;
    delete_registration(&c, &reg_id).await;
    mock.shutdown().await;
    scope.finish().await.expect("cleanup failed");
}
