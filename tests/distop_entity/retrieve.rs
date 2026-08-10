#![allow(clippy::expect_used, clippy::unwrap_used)]
//! Distributed entity retrieve tests.
//!
//! Tests retrieving single entities across context sources with different registration modes.

use reqwest::{Method, StatusCode};
use serial_test::serial;

use crate::{
    context_source_adapter::ContextSourceAdapter as MockServer,
    helpers::{
        clean_database,
        client,
        create_entity_local,
        create_registration,
        delete_entity,
        delete_registration,
        get_entity,
        jsonld_link_value,
        unique_id,
        vehicle_entity,
        vehicle_entity_second,
        vehicle_entity_speed_only,
        vehicle_registration,
        vehicle_registration_speed_property,
        vehicle_registration_with_ops,
    },
};

// ---------------------------------------------------------------------------
// Auxiliary mode tests
// ---------------------------------------------------------------------------

/// Retrieve entity merges local and remote attributes in auxiliary mode.
#[tokio::test]
#[serial]
async fn distop_entity_retrieve_auxiliary_merges_local_and_remote_attributes() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let mock = MockServer::start().await;

    let entity_id = unique_id("urn:ngsi-ld:Vehicle:retrieve-aux");
    let reg_id = unique_id("urn:ngsi-ld:Registration:retrieve-aux");

    // Create local entity (brandName=Mercedes, isParked)
    let local_entity = vehicle_entity(&entity_id);
    let (status, _) = create_entity_local(&c, &local_entity).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    // Create auxiliary registration
    let reg = vehicle_registration(&reg_id, Some(&entity_id), &mock.url(""), Some("auxiliary"));
    let (status, _) = create_registration(&c, &reg).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    // Mock returns entity with brandName=BMW and isParked2
    let mock_entity = vehicle_entity_second(&entity_id);
    mock.set_stub_with_body(Method::GET, &format!("/ngsi-ld/v1/entities/{entity_id}"), 200, &mock_entity)
        .await;

    // Retrieve entity
    let (status, _, body) = get_entity(&c, &entity_id, "", &jsonld_link_value()).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    let obj = body.as_object().unwrap();
    let keys: Vec<_> = obj.keys().collect();
    assert!(obj.contains_key("brandName"), "expected brandName in {keys:?}");
    assert!(obj.contains_key("isParked"), "expected isParked in {keys:?}");
    assert!(obj.contains_key("isParked2"), "expected isParked2 in {keys:?}");

    delete_entity(&c, &entity_id).await;
    delete_registration(&c, &reg_id).await;
    mock.shutdown().await;
}

// ---------------------------------------------------------------------------
// Exclusive mode tests
// ---------------------------------------------------------------------------

/// Retrieve entity from exclusive registration returns remote attributes.
#[tokio::test]
#[serial]
async fn distop_entity_retrieve_exclusive_returns_remote_attributes() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let mock = MockServer::start().await;

    let entity_id = unique_id("urn:ngsi-ld:Vehicle:retrieve-exc");
    let reg_id = unique_id("urn:ngsi-ld:Registration:retrieve-exc");

    // Create local entity (no speed attribute)
    let local_entity = vehicle_entity(&entity_id);
    let (status, _) = create_entity_local(&c, &local_entity).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    // Create exclusive registration with retrieveEntity operation
    let reg = vehicle_registration_speed_property(&reg_id, &entity_id, &mock.url("/broker1"), Some("exclusive"), Some(&["retrieveEntity"]));
    let (status, _) = create_registration(&c, &reg).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    // Mock returns entity with speed attribute
    let mock_entity = vehicle_entity_speed_only(&entity_id);
    mock.set_stub_with_body(Method::GET, &format!("/broker1/ngsi-ld/v1/entities/{entity_id}"), 200, &mock_entity)
        .await;

    // Retrieve entity
    let (status, _, body) = get_entity(&c, &entity_id, "", &jsonld_link_value()).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    let obj = body.as_object().unwrap();
    assert!(
        obj.contains_key("speed"),
        "expected speed from exclusive source, got keys: {:?}",
        obj.keys().collect::<Vec<_>>()
    );

    let count = mock.get_count(Method::GET, &format!("/broker1/ngsi-ld/v1/entities/{entity_id}")).await;
    assert!(count > 0, "mock should have been called");

    delete_entity(&c, &entity_id).await;
    delete_registration(&c, &reg_id).await;
    mock.shutdown().await;
}

/// Retrieve entity with exclusive registration and local=true excludes remote attributes.
#[tokio::test]
#[serial]
async fn distop_entity_retrieve_exclusive_local_true_excludes_remote() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let mock = MockServer::start().await;

    let entity_id = unique_id("urn:ngsi-ld:Vehicle:retrieve-exc-local");
    let reg_id = unique_id("urn:ngsi-ld:Registration:retrieve-exc-local");

    // Create local entity
    let local_entity = vehicle_entity(&entity_id);
    let (status, _) = create_entity_local(&c, &local_entity).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    // Create exclusive registration
    let reg = vehicle_registration_speed_property(&reg_id, &entity_id, &mock.url("/broker1"), Some("exclusive"), None);
    let (status, _) = create_registration(&c, &reg).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    // Mock returns entity with speed
    let mock_entity = vehicle_entity_speed_only(&entity_id);
    mock.set_stub_with_body(Method::GET, &format!("/broker1/ngsi-ld/v1/entities/{entity_id}"), 200, &mock_entity)
        .await;

    // Retrieve with local=true
    let (status, _, body) = get_entity(&c, &entity_id, "local=true", &jsonld_link_value()).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    assert!(
        !body.as_object().unwrap().contains_key("speed"),
        "local=true should not include exclusive speed attribute"
    );

    delete_entity(&c, &entity_id).await;
    delete_registration(&c, &reg_id).await;
    mock.shutdown().await;
}

// ---------------------------------------------------------------------------
// Inclusive mode tests
// ---------------------------------------------------------------------------

/// Retrieve entity from inclusive registration returns entity from remote.
#[tokio::test]
#[serial]
async fn distop_entity_retrieve_inclusive_returns_remote_entity() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let mock = MockServer::start().await;

    let entity_id = unique_id("urn:ngsi-ld:Vehicle:retrieve-inc");
    let reg_id = unique_id("urn:ngsi-ld:Registration:retrieve-inc");

    // Create inclusive registration (no mode = inclusive default)
    let reg = vehicle_registration(&reg_id, Some(&entity_id), &mock.url(""), None);
    let (status, _) = create_registration(&c, &reg).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    // Mock returns entity
    let mock_entity = vehicle_entity(&entity_id);
    mock.set_stub_with_body(Method::GET, &format!("/ngsi-ld/v1/entities/{entity_id}"), 200, &mock_entity)
        .await;

    // Retrieve entity
    let (status, _, _) = get_entity(&c, &entity_id, "", &jsonld_link_value()).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    delete_registration(&c, &reg_id).await;
    mock.shutdown().await;
}

/// Retrieve with local=true and inclusive registration returns 404 when entity is only remote.
#[tokio::test]
#[serial]
async fn distop_entity_retrieve_inclusive_local_true_returns_404() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let mock = MockServer::start().await;

    let entity_id = unique_id("urn:ngsi-ld:Vehicle:retrieve-inc-local");
    let reg_id = unique_id("urn:ngsi-ld:Registration:retrieve-inc-local");

    // Create inclusive registration (entity only on remote)
    let reg = vehicle_registration(&reg_id, Some(&entity_id), &mock.url(""), None);
    let (status, _) = create_registration(&c, &reg).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    // Mock stub (should not be called)
    let mock_entity = vehicle_entity(&entity_id);
    mock.set_stub_with_body(Method::GET, &format!("/ngsi-ld/v1/entities/{entity_id}"), 200, &mock_entity)
        .await;

    // Retrieve with local=true returns 404
    let (status, _, _) = get_entity(&c, &entity_id, "local=true", &jsonld_link_value()).await;
    pretty_assertions::assert_eq!(status, StatusCode::NOT_FOUND);

    // Verify no request was forwarded
    pretty_assertions::assert_eq!(mock.get_total_count().await, 0);

    delete_registration(&c, &reg_id).await;
    mock.shutdown().await;
}

/// Inclusive registration with retrieveEntity operation forwards correctly.
#[tokio::test]
#[serial]
async fn distop_entity_retrieve_inclusive_with_retrieve_entity_op() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let mock = MockServer::start().await;

    let entity_id = unique_id("urn:ngsi-ld:Vehicle:retrieve-inc-op1");
    let reg_id = unique_id("urn:ngsi-ld:Registration:retrieve-inc-op1");

    // Create inclusive registration with retrieveEntity operation
    let reg = vehicle_registration_with_ops(&reg_id, Some(&entity_id), &mock.url(""), None, &["retrieveEntity"]);
    let (status, _) = create_registration(&c, &reg).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    // Mock returns entity
    let mock_entity = vehicle_entity(&entity_id);
    mock.set_stub_with_body(Method::GET, &format!("/ngsi-ld/v1/entities/{entity_id}"), 200, &mock_entity)
        .await;

    // Retrieve entity
    let (status, _, _) = get_entity(&c, &entity_id, "", &jsonld_link_value()).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    delete_registration(&c, &reg_id).await;
    mock.shutdown().await;
}

/// Retrieval does not use the query-entity capability.
#[tokio::test]
#[serial]
async fn distop_entity_retrieve_with_only_query_entity_does_not_forward() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let mock = MockServer::start().await;

    let entity_id = unique_id("urn:ngsi-ld:Vehicle:retrieve-inc-op2");
    let reg_id = unique_id("urn:ngsi-ld:Registration:retrieve-inc-op2");

    // Create inclusive registration with queryEntity operation
    let reg = vehicle_registration_with_ops(&reg_id, Some(&entity_id), &mock.url(""), None, &["queryEntity"]);
    let (status, _) = create_registration(&c, &reg).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let mock_entity = vehicle_entity(&entity_id);
    mock.set_stub_with_body(Method::GET, &format!("/ngsi-ld/v1/entities/{entity_id}"), 200, &mock_entity)
        .await;

    let (status, _, _) = get_entity(&c, &entity_id, "", &jsonld_link_value()).await;
    pretty_assertions::assert_eq!(status, StatusCode::NOT_FOUND);
    pretty_assertions::assert_eq!(mock.get_count(Method::GET, &format!("/ngsi-ld/v1/entities/{entity_id}")).await, 0);

    delete_registration(&c, &reg_id).await;
    mock.shutdown().await;
}

/// Retrieval does not use the batch-query capability.
#[tokio::test]
#[serial]
async fn distop_entity_retrieve_with_only_query_batch_does_not_forward() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let mock = MockServer::start().await;

    let entity_id = unique_id("urn:ngsi-ld:Vehicle:retrieve-inc-op3");
    let reg_id = unique_id("urn:ngsi-ld:Registration:retrieve-inc-op3");

    // Create inclusive registration with queryBatch operation
    let reg = vehicle_registration_with_ops(&reg_id, Some(&entity_id), &mock.url(""), None, &["queryBatch"]);
    let (status, _) = create_registration(&c, &reg).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let mock_entity = vehicle_entity(&entity_id);
    mock.set_stub_with_body(Method::GET, &format!("/ngsi-ld/v1/entities/{entity_id}"), 200, &mock_entity)
        .await;

    let (status, _, _) = get_entity(&c, &entity_id, "", &jsonld_link_value()).await;
    pretty_assertions::assert_eq!(status, StatusCode::NOT_FOUND);
    pretty_assertions::assert_eq!(mock.get_count(Method::GET, &format!("/ngsi-ld/v1/entities/{entity_id}")).await, 0);

    delete_registration(&c, &reg_id).await;
    mock.shutdown().await;
}

// ---------------------------------------------------------------------------
// Redirect mode tests
// ---------------------------------------------------------------------------

/// Retrieve entity with two redirect registrations merges attributes from both.
#[tokio::test]
#[serial]
async fn distop_entity_retrieve_redirect_merges_two_brokers() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let mock = MockServer::start().await;

    let entity_id = unique_id("urn:ngsi-ld:Vehicle:retrieve-red");
    let reg_id1 = unique_id("urn:ngsi-ld:Registration:retrieve-red-1");
    let reg_id2 = unique_id("urn:ngsi-ld:Registration:retrieve-red-2");

    // Create two redirect registrations
    let reg1 = vehicle_registration_with_ops(&reg_id1, Some(&entity_id), &mock.url("/broker1"), Some("redirect"), &["redirectionOps"]);
    let (status, _) = create_registration(&c, &reg1).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let reg2 = vehicle_registration_with_ops(&reg_id2, Some(&entity_id), &mock.url("/broker2"), Some("redirect"), &["redirectionOps"]);
    let (status, _) = create_registration(&c, &reg2).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    // broker1 returns entity with isParked
    let mock_entity1 = vehicle_entity(&entity_id);
    mock.set_stub_with_body(Method::GET, &format!("/broker1/ngsi-ld/v1/entities/{entity_id}"), 200, &mock_entity1)
        .await;

    // broker2 returns entity with isParked2
    let mock_entity2 = vehicle_entity_second(&entity_id);
    mock.set_stub_with_body(Method::GET, &format!("/broker2/ngsi-ld/v1/entities/{entity_id}"), 200, &mock_entity2)
        .await;

    // Retrieve entity
    let (status, _, body) = get_entity(&c, &entity_id, "", &jsonld_link_value()).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    let obj = body.as_object().unwrap();
    let keys: Vec<_> = obj.keys().collect();
    assert!(obj.contains_key("isParked"), "expected isParked in {keys:?}");
    assert!(obj.contains_key("isParked2"), "expected isParked2 in {keys:?}");

    delete_registration(&c, &reg_id1).await;
    delete_registration(&c, &reg_id2).await;
    mock.shutdown().await;
}

/// Retrieve with redirect registration and local=true returns 404.
#[tokio::test]
#[serial]
async fn distop_entity_retrieve_redirect_local_true_returns_404() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let mock = MockServer::start().await;

    let entity_id = unique_id("urn:ngsi-ld:Vehicle:retrieve-red-local");
    let reg_id1 = unique_id("urn:ngsi-ld:Registration:retrieve-red-local-1");
    let reg_id2 = unique_id("urn:ngsi-ld:Registration:retrieve-red-local-2");

    // Create two redirect registrations
    let reg1 = vehicle_registration_with_ops(&reg_id1, Some(&entity_id), &mock.url("/broker1"), Some("redirect"), &["redirectionOps"]);
    let (status, _) = create_registration(&c, &reg1).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let reg2 = vehicle_registration_with_ops(&reg_id2, Some(&entity_id), &mock.url("/broker2"), Some("redirect"), &["redirectionOps"]);
    let (status, _) = create_registration(&c, &reg2).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    // Set mock stubs
    mock.set_stub(Method::GET, &format!("/broker1/ngsi-ld/v1/entities/{entity_id}"), 200).await;
    mock.set_stub(Method::GET, &format!("/broker2/ngsi-ld/v1/entities/{entity_id}"), 200).await;

    // Retrieve with local=true returns 404
    let (status, _, _) = get_entity(&c, &entity_id, "local=true", &jsonld_link_value()).await;
    pretty_assertions::assert_eq!(status, StatusCode::NOT_FOUND);

    // Verify no requests forwarded
    pretty_assertions::assert_eq!(mock.get_count(Method::GET, &format!("/broker1/ngsi-ld/v1/entities/{entity_id}")).await, 0);
    pretty_assertions::assert_eq!(mock.get_count(Method::GET, &format!("/broker2/ngsi-ld/v1/entities/{entity_id}")).await, 0);

    delete_registration(&c, &reg_id1).await;
    delete_registration(&c, &reg_id2).await;
    mock.shutdown().await;
}
