#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]
//! Distributed entity query tests.
//!
//! Tests querying entities across context sources with different registration modes,
//! entityMap, and splitEntities functionality.

use andromeda_test_harness::configuration::RUNTIME_BROKER_BASE as BASE_URL;
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};
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
        delete_registration,
        jsonld_link_value,
        query_entities,
        query_entities_post,
        unique_id,
        vehicle_entity,
        vehicle_entity_different_ld,
        vehicle_entity_second,
        vehicle_entity_second_ld,
        vehicle_entity_speed_only,
        vehicle_entity_with_scope,
        vehicle_entity_with_speed,
        vehicle_registration,
        vehicle_registration_speed_property,
        vehicle_registration_with_ops,
    },
};
use urlencoding::encode;

/// Query entities with auxiliary mode merges local and remote attributes.
#[tokio::test]
#[serial]
async fn distop_entity_query_auxiliary_merges_local_and_remote() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let mock = MockServer::start().await;

    let entity_id = unique_id("urn:ngsi-ld:Vehicle:query-aux");
    let reg_id = unique_id("urn:ngsi-ld:Registration:query-aux");

    let entity = vehicle_entity_second_ld(&entity_id);
    let (status, _) = create_entity_local(&c, &entity).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let reg = vehicle_registration(&reg_id, Some(&entity_id), &mock.url(""), Some("auxiliary"));
    let (status, _) = create_registration(&c, &reg).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let mock_entity = vehicle_entity(&entity_id);
    let mock_array = Value::Array(vec![mock_entity]);
    mock.set_stub_with_body(Method::GET, "/ngsi-ld/v1/entities", 200, &mock_array).await;

    let (status, _, body) = query_entities(&c, "type=Vehicle", &jsonld_link_value()).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    let entries = body.as_array().expect("response should be array");
    assert!(!entries.is_empty(), "should return at least one entity");

    let first = &entries[0];
    let obj = first.as_object().unwrap();
    let keys: Vec<_> = obj.keys().collect();
    assert!(obj.contains_key("brandName"), "expected brandName in {keys:?}");
    assert!(obj.contains_key("isParked"), "expected isParked in {keys:?}");
    assert!(obj.contains_key("isParked2"), "expected isParked2 in {keys:?}");

    delete_entity(&c, &entity_id).await;
    delete_registration(&c, &reg_id).await;
    mock.shutdown().await;
}

/// Query entities with exclusive mode returns remote attributes.
#[tokio::test]
#[serial]
async fn distop_entity_query_exclusive_returns_remote() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let mock = MockServer::start().await;

    let entity_id = unique_id("urn:ngsi-ld:Vehicle:query-exc");
    let reg_id = unique_id("urn:ngsi-ld:Registration:query-exc");

    let entity = vehicle_entity(&entity_id);
    let (status, _) = create_entity_local(&c, &entity).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let reg = vehicle_registration_speed_property(&reg_id, &entity_id, &mock.url("/broker1"), Some("exclusive"), Some(&["queryEntity"]));
    let (status, _) = create_registration(&c, &reg).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let mock_entity = vehicle_entity_speed_only(&entity_id);
    let mock_array = Value::Array(vec![mock_entity]);
    mock.set_stub_with_body(Method::GET, "/broker1/ngsi-ld/v1/entities", 200, &mock_array).await;

    let (status, _, body) = query_entities(&c, "type=Vehicle", &jsonld_link_value()).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    let entries = body.as_array().expect("response should be array");
    let entity = entries.iter().find(|e| e["id"] == entity_id).unwrap();
    let obj = entity.as_object().unwrap();
    assert!(
        obj.contains_key("speed"),
        "expected speed from exclusive source, got keys: {:?}",
        obj.keys().collect::<Vec<_>>()
    );

    let count = mock.get_count(Method::GET, "/broker1/ngsi-ld/v1/entities");
    assert!(count > 0, "mock should have been called");

    delete_entity(&c, &entity_id).await;
    delete_registration(&c, &reg_id).await;
    mock.shutdown().await;
}

/// Exclusive with queryEntity operation forwards correctly.
#[tokio::test]
#[serial]
async fn distop_entity_query_exclusive_with_query_entity_op() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let mock = MockServer::start().await;

    let entity_id = unique_id("urn:ngsi-ld:Vehicle:query-exc-op1");
    let reg_id = unique_id("urn:ngsi-ld:Registration:query-exc-op1");

    let entity = vehicle_entity(&entity_id);
    let (status, _) = create_entity_local(&c, &entity).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let reg = vehicle_registration_speed_property(&reg_id, &entity_id, &mock.url("/broker1"), Some("exclusive"), Some(&["queryEntity"]));
    let (status, _) = create_registration(&c, &reg).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let mock_entity = vehicle_entity_speed_only(&entity_id);
    let mock_array = Value::Array(vec![mock_entity]);
    mock.set_stub_with_body(Method::GET, "/broker1/ngsi-ld/v1/entities", 200, &mock_array).await;

    let (status, _, body) = query_entities(&c, "type=Vehicle", &jsonld_link_value()).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    let entries = body.as_array().expect("response should be array");
    assert!(entries.iter().any(|e| e["id"] == entity_id));

    delete_entity(&c, &entity_id).await;
    delete_registration(&c, &reg_id).await;
    mock.shutdown().await;
}

/// Exclusive with queryBatch operation forwards correctly.
#[tokio::test]
#[serial]
async fn distop_entity_query_exclusive_with_query_batch_op() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let mock = MockServer::start().await;

    let entity_id = unique_id("urn:ngsi-ld:Vehicle:query-exc-op2");
    let reg_id = unique_id("urn:ngsi-ld:Registration:query-exc-op2");

    let entity = vehicle_entity(&entity_id);
    let (status, _) = create_entity_local(&c, &entity).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let reg = vehicle_registration_speed_property(&reg_id, &entity_id, &mock.url("/broker1"), Some("exclusive"), Some(&["queryBatch"]));
    let (status, _) = create_registration(&c, &reg).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    // The broker always forwards queries as GET /entities
    let mock_entity = vehicle_entity_speed_only(&entity_id);
    let mock_array = Value::Array(vec![mock_entity]);
    mock.set_stub_with_body(Method::GET, "/broker1/ngsi-ld/v1/entities", 200, &mock_array).await;

    let (status, _, body) = query_entities(&c, "type=Vehicle", &jsonld_link_value()).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    let entries = body.as_array().expect("response should be array");
    assert!(entries.iter().any(|e| e["id"] == entity_id));

    delete_entity(&c, &entity_id).await;
    delete_registration(&c, &reg_id).await;
    mock.shutdown().await;
}

/// Query entities with redirect mode returns entity from remote.
#[tokio::test]
#[serial]
async fn distop_entity_query_redirect_returns_remote() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let mock = MockServer::start().await;

    let entity_id = unique_id("urn:ngsi-ld:Vehicle:query-red");
    let reg_id = unique_id("urn:ngsi-ld:Registration:query-red");

    let reg = vehicle_registration(&reg_id, Some(&entity_id), &mock.url("/broker1"), Some("redirect"));
    let (status, _) = create_registration(&c, &reg).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let mock_entity = vehicle_entity(&entity_id);
    let mock_array = Value::Array(vec![mock_entity]);
    mock.set_stub_with_body(Method::GET, "/broker1/ngsi-ld/v1/entities", 200, &mock_array).await;

    let (status, _, body) = query_entities(&c, "type=Vehicle", &jsonld_link_value()).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    let entries = body.as_array().expect("response should be array");
    assert!(entries.iter().any(|e| e["id"] == entity_id));

    delete_registration(&c, &reg_id).await;
    mock.shutdown().await;
}

/// Redirect with queryEntity operation forwards correctly.
#[tokio::test]
#[serial]
async fn distop_entity_query_redirect_with_query_entity_op() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let mock = MockServer::start().await;

    let entity_id = unique_id("urn:ngsi-ld:Vehicle:query-red-op1");
    let reg_id = unique_id("urn:ngsi-ld:Registration:query-red-op1");

    let reg = vehicle_registration_with_ops(&reg_id, Some(&entity_id), &mock.url("/broker1"), Some("redirect"), &["queryEntity"]);
    let (status, _) = create_registration(&c, &reg).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let mock_entity = vehicle_entity(&entity_id);
    let mock_array = Value::Array(vec![mock_entity]);
    mock.set_stub_with_body(Method::GET, "/broker1/ngsi-ld/v1/entities", 200, &mock_array).await;

    let (status, _, body) = query_entities(&c, "type=Vehicle", &jsonld_link_value()).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    let entries = body.as_array().expect("response should be array");
    assert!(entries.iter().any(|e| e["id"] == entity_id));

    delete_registration(&c, &reg_id).await;
    mock.shutdown().await;
}

/// A GET entity query does not use the batch-query capability.
#[tokio::test]
#[serial]
async fn distop_entity_query_redirect_with_only_query_batch_does_not_forward() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let mock = MockServer::start().await;

    let entity_id = unique_id("urn:ngsi-ld:Vehicle:query-red-op2");
    let reg_id = unique_id("urn:ngsi-ld:Registration:query-red-op2");

    let reg = vehicle_registration_with_ops(&reg_id, Some(&entity_id), &mock.url("/broker1"), Some("redirect"), &["queryBatch"]);
    let (status, _) = create_registration(&c, &reg).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    // The broker always forwards queries as GET /entities
    let mock_entity = vehicle_entity(&entity_id);
    let mock_array = Value::Array(vec![mock_entity]);
    mock.set_stub_with_body(Method::GET, "/broker1/ngsi-ld/v1/entities", 200, &mock_array).await;

    let (status, _, body) = query_entities(&c, "type=Vehicle", &jsonld_link_value()).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    let entries = body.as_array().expect("response should be array");
    assert!(
        entries.is_empty(),
        "a registration offering only queryBatch is never forwarded a GET query: {entries:?}"
    );
    pretty_assertions::assert_eq!(mock.get_count(Method::GET, "/broker1/ngsi-ld/v1/entities"), 0);

    delete_registration(&c, &reg_id).await;
    mock.shutdown().await;
}

/// Query by type with inclusive registration returns entity from remote.
#[tokio::test]
#[serial]
async fn distop_entity_query_inclusive_by_type() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let mock = MockServer::start().await;

    let entity_id = unique_id("urn:ngsi-ld:Vehicle:query-inc-type");
    let reg_id = unique_id("urn:ngsi-ld:Registration:query-inc-type");

    let reg = vehicle_registration(&reg_id, Some(&entity_id), &mock.url(""), None);
    let (status, _) = create_registration(&c, &reg).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let mock_entity = vehicle_entity(&entity_id);
    let mock_array = Value::Array(vec![mock_entity]);
    mock.set_stub_with_body(Method::GET, "/ngsi-ld/v1/entities", 200, &mock_array).await;

    let (status, _, body) = query_entities(&c, "type=Vehicle", &jsonld_link_value()).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    let entries = body.as_array().expect("response should be array");
    assert!(entries.iter().any(|e| e["id"] == entity_id));

    delete_registration(&c, &reg_id).await;
    mock.shutdown().await;
}

/// Query by type with missing attribute returns empty result.
#[tokio::test]
#[serial]
async fn distop_entity_query_inclusive_missing_attr_returns_empty() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let mock = MockServer::start().await;

    let entity_id = unique_id("urn:ngsi-ld:Vehicle:query-inc-attr");
    let reg_id = unique_id("urn:ngsi-ld:Registration:query-inc-attr");

    let reg = vehicle_registration(&reg_id, Some(&entity_id), &mock.url(""), None);
    let (status, _) = create_registration(&c, &reg).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let mock_entity = vehicle_entity(&entity_id);
    let mock_array = Value::Array(vec![mock_entity]);
    mock.set_stub_with_body(Method::GET, "/ngsi-ld/v1/entities", 200, &mock_array).await;

    let (status, _, body) = query_entities(&c, "type=Vehicle&attrs=speed", &jsonld_link_value()).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    let entries = body.as_array().expect("response should be array");
    assert!(entries.is_empty(), "no entity should have speed attribute, got {body:?}");

    delete_registration(&c, &reg_id).await;
    mock.shutdown().await;
}

/// Query by type and attribute with two sources returns entity with requested attribute.
#[tokio::test]
#[serial]
async fn distop_entity_query_inclusive_by_type_and_attr_two_sources() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let mock = MockServer::start().await;

    let entity_id_local = unique_id("urn:ngsi-ld:Vehicle:query-inc-local");
    let entity_id_remote = unique_id("urn:ngsi-ld:Vehicle:query-inc-remote");
    let reg_id = unique_id("urn:ngsi-ld:Registration:query-inc-attr2");

    let entity = vehicle_entity(&entity_id_local);
    let (status, _) = create_entity_local(&c, &entity).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let reg = vehicle_registration(&reg_id, Some(&entity_id_remote), &mock.url(""), None);
    let (status, _) = create_registration(&c, &reg).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let mock_entity = vehicle_entity_second(&entity_id_remote);
    let mock_array = Value::Array(vec![mock_entity]);
    mock.set_stub_with_body(Method::GET, "/ngsi-ld/v1/entities", 200, &mock_array).await;

    let (status, _, body) = query_entities(&c, "type=Vehicle&attrs=isParked2", &jsonld_link_value()).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    let entries = body.as_array().expect("response should be array");
    assert!(
        entries.iter().any(|e| e["id"] == entity_id_remote),
        "should contain remote entity with isParked2"
    );

    delete_entity(&c, &entity_id_local).await;
    delete_registration(&c, &reg_id).await;
    mock.shutdown().await;
}

/// Query by type with attribute in neither source returns empty.
#[tokio::test]
#[serial]
async fn distop_entity_query_inclusive_attr_in_neither_returns_empty() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let mock = MockServer::start().await;

    let entity_id_local = unique_id("urn:ngsi-ld:Vehicle:query-inc-neither-local");
    let entity_id_remote = unique_id("urn:ngsi-ld:Vehicle:query-inc-neither-remote");
    let reg_id = unique_id("urn:ngsi-ld:Registration:query-inc-neither");

    let entity = vehicle_entity_second_ld(&entity_id_local);
    let (status, _) = create_entity_local(&c, &entity).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let reg = vehicle_registration(&reg_id, Some(&entity_id_remote), &mock.url(""), None);
    let (status, _) = create_registration(&c, &reg).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let mock_entity = vehicle_entity(&entity_id_remote);
    let mock_array = Value::Array(vec![mock_entity]);
    mock.set_stub_with_body(Method::GET, "/ngsi-ld/v1/entities", 200, &mock_array).await;

    let (status, _, body) = query_entities(&c, "type=Vehicle&attrs=speed", &jsonld_link_value()).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    let entries = body.as_array().expect("response should be array");
    assert!(entries.is_empty(), "neither entity has speed attribute, got {body:?}");

    delete_entity(&c, &entity_id_local).await;
    delete_registration(&c, &reg_id).await;
    mock.shutdown().await;
}

/// Query merges same-ID entity from both local and remote sources.
#[tokio::test]
#[serial]
async fn distop_entity_query_inclusive_merges_same_id() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let mock = MockServer::start().await;

    let entity_id = unique_id("urn:ngsi-ld:Vehicle:query-inc-merge");
    let reg_id = unique_id("urn:ngsi-ld:Registration:query-inc-merge");

    let entity = vehicle_entity_different_ld(&entity_id);
    let (status, _) = create_entity_local(&c, &entity).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let reg = vehicle_registration(&reg_id, Some(&entity_id), &mock.url(""), None);
    let (status, _) = create_registration(&c, &reg).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let mock_entity = vehicle_entity(&entity_id);
    let mock_array = Value::Array(vec![mock_entity]);
    mock.set_stub_with_body(Method::GET, "/ngsi-ld/v1/entities", 200, &mock_array).await;

    let (status, _, body) = query_entities(&c, "type=Vehicle", &jsonld_link_value()).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    let entries = body.as_array().expect("response should be array");
    let entity = entries.iter().find(|e| e["id"] == entity_id).unwrap();
    let obj = entity.as_object().unwrap();
    let keys: Vec<_> = obj.keys().collect();
    assert!(obj.contains_key("brandName"), "expected brandName in {keys:?}");
    assert!(obj.contains_key("speed"), "expected speed in {keys:?}");
    assert!(obj.contains_key("isParked"), "expected isParked in {keys:?}");
    assert!(obj.contains_key("isParked2"), "expected isParked2 in {keys:?}");

    delete_entity(&c, &entity_id).await;
    delete_registration(&c, &reg_id).await;
    mock.shutdown().await;
}

/// Query with local=true and inclusive registration (no local entities) returns empty.
#[tokio::test]
#[serial]
async fn distop_entity_query_inclusive_local_true_returns_empty() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let mock = MockServer::start().await;

    let entity_id = unique_id("urn:ngsi-ld:Vehicle:query-inc-local");
    let reg_id = unique_id("urn:ngsi-ld:Registration:query-inc-local");

    let reg = vehicle_registration(&reg_id, Some(&entity_id), &mock.url(""), None);
    let (status, _) = create_registration(&c, &reg).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let mock_entity = vehicle_entity(&entity_id);
    let mock_array = Value::Array(vec![mock_entity]);
    mock.set_stub_with_body(Method::GET, "/ngsi-ld/v1/entities", 200, &mock_array).await;

    let (status, _, body) = query_entities(&c, "type=Vehicle&local=true", &jsonld_link_value()).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    let entries = body.as_array().expect("response should be array");
    assert!(entries.is_empty(), "local=true should return empty when no local entities");

    delete_registration(&c, &reg_id).await;
    mock.shutdown().await;
}

/// Query by entity ID with queryEntity operation.
#[tokio::test]
#[serial]
async fn distop_entity_query_inclusive_by_id_with_query_entity_op() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let mock = MockServer::start().await;

    let entity_id = unique_id("urn:ngsi-ld:Vehicle:query-inc-id-op1");
    let reg_id = unique_id("urn:ngsi-ld:Registration:query-inc-id-op1");

    let reg = vehicle_registration_with_ops(&reg_id, Some(&entity_id), &mock.url(""), None, &["queryEntity"]);
    let (status, _) = create_registration(&c, &reg).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let mock_entity = vehicle_entity(&entity_id);
    let mock_array = Value::Array(vec![mock_entity]);
    mock.set_stub_with_body(Method::GET, "/ngsi-ld/v1/entities", 200, &mock_array).await;

    let query = format!("type=Vehicle&id={}", encode(&entity_id));
    let (status, _, body) = query_entities(&c, &query, &jsonld_link_value()).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    let entries = body.as_array().expect("response should be array");
    assert!(entries.iter().any(|e| e["id"] == entity_id));

    delete_registration(&c, &reg_id).await;
    mock.shutdown().await;
}

/// A GET query by entity ID does not use the batch-query capability.
#[tokio::test]
#[serial]
async fn distop_entity_query_inclusive_by_id_with_only_query_batch_does_not_forward() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let mock = MockServer::start().await;

    let entity_id = unique_id("urn:ngsi-ld:Vehicle:query-inc-id-op2");
    let reg_id = unique_id("urn:ngsi-ld:Registration:query-inc-id-op2");

    let reg = vehicle_registration_with_ops(&reg_id, Some(&entity_id), &mock.url(""), None, &["queryBatch"]);
    let (status, _) = create_registration(&c, &reg).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    // The broker always forwards queries as GET /entities
    let mock_entity = vehicle_entity(&entity_id);
    let mock_array = Value::Array(vec![mock_entity]);
    mock.set_stub_with_body(Method::GET, "/ngsi-ld/v1/entities", 200, &mock_array).await;

    let query = format!("type=Vehicle&id={}", encode(&entity_id));
    let (status, _, body) = query_entities(&c, &query, &jsonld_link_value()).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    let entries = body.as_array().expect("response should be array");
    assert!(
        entries.is_empty(),
        "a registration offering only queryBatch is never forwarded a GET query: {entries:?}"
    );
    pretty_assertions::assert_eq!(mock.get_count(Method::GET, "/ngsi-ld/v1/entities"), 0);

    delete_registration(&c, &reg_id).await;
    mock.shutdown().await;
}

/// A POST batch query reaches a source that advertises `queryBatch` through a
/// forwarded POST batch query at `/entityOperations/query`, the batch endpoint it
/// advertises (clause 4.3.6.1; ETSI conformance `D011_03_inc_02`).
#[tokio::test]
#[serial]
async fn distop_entity_query_post_with_query_batch_op() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let mock = MockServer::start().await;

    let entity_id = unique_id("urn:ngsi-ld:Vehicle:query-post-op1");
    let reg_id = unique_id("urn:ngsi-ld:Registration:query-post-op1");

    let reg = vehicle_registration_with_ops(&reg_id, Some(&entity_id), &mock.url(""), None, &["queryBatch"]);
    let (status, _) = create_registration(&c, &reg).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    // A queryBatch source is contacted through the POST batch endpoint it advertises.
    let mock_array = Value::Array(vec![vehicle_entity(&entity_id)]);
    mock.set_stub_with_body(Method::POST, "/ngsi-ld/v1/entityOperations/query", 200, &mock_array)
        .await;

    let query_body = json!({
        "type": "Query",
        "entities": [{"id": entity_id, "type": "Vehicle"}]
    });
    let (status, _, body) = query_entities_post(&c, &query_body, &jsonld_link_value()).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    let entries = body.as_array().expect("response should be array");
    assert!(entries.iter().any(|e| e["id"] == entity_id));
    assert!(
        mock.get_count(Method::POST, "/ngsi-ld/v1/entityOperations/query") > 0,
        "the batch query is forwarded to the queryBatch source"
    );

    delete_registration(&c, &reg_id).await;
    mock.shutdown().await;
}

/// A POST batch query reaches a source that advertises only `queryEntity` through a
/// forwarded GET query (clause 6.23.1: the GET and POST bindings are the same "Query
/// Entities" operation; clause 4.3.6.1: reach the source through an operation it
/// advertises). The source is queried at `/entities`.
#[tokio::test]
#[serial]
async fn distop_entity_query_post_with_only_query_entity_forwards_as_query() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let mock = MockServer::start().await;

    let entity_id = unique_id("urn:ngsi-ld:Vehicle:query-post-op2");
    let reg_id = unique_id("urn:ngsi-ld:Registration:query-post-op2");

    // A registration advertising only queryEntity: the POST batch query is downgraded
    // to a GET query (clause 6.23.1).
    let reg = vehicle_registration_with_ops(&reg_id, Some(&entity_id), &mock.url(""), None, &["queryEntity"]);
    let (status, _) = create_registration(&c, &reg).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let mock_array = Value::Array(vec![vehicle_entity(&entity_id)]);
    mock.set_stub_with_body(Method::GET, "/ngsi-ld/v1/entities", 200, &mock_array).await;

    let query_body = json!({
        "type": "Query",
        "entities": [{"id": entity_id, "type": "Vehicle"}]
    });
    let (status, _, body) = query_entities_post(&c, &query_body, &jsonld_link_value()).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    let entries = body.as_array().expect("response should be array");
    assert!(
        entries.iter().any(|e| e["id"] == entity_id),
        "the queryEntity source serves the batch query: {entries:?}"
    );
    assert!(
        mock.get_count(Method::GET, "/ngsi-ld/v1/entities") > 0,
        "the batch query is forwarded as a GET query"
    );

    delete_registration(&c, &reg_id).await;
    mock.shutdown().await;
}

/// `EntityMap` from distributed query includes both local and forwarded entity IDs.
#[tokio::test]
#[serial]
async fn distop_entity_map_includes_forwarded_entity_ids() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let mock = MockServer::start().await;

    let local_id = unique_id("urn:ngsi-ld:Vehicle:map-local");
    let remote_id = unique_id("urn:ngsi-ld:Vehicle:map-remote");
    let reg_id = unique_id("urn:ngsi-ld:Registration:map");

    let entity = vehicle_entity(&local_id);
    let (status, _) = create_entity(&c, &entity).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let mock_entity = vehicle_entity(&remote_id);
    let mock_array = Value::Array(vec![mock_entity]);
    mock.set_stub_with_body(Method::GET, "/ngsi-ld/v1/entities", 200, &mock_array).await;

    let reg = vehicle_registration(&reg_id, None, &mock.url(""), Some("auxiliary"));
    let (status, _) = create_registration(&c, &reg).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let (status, headers, _) = query_entities(&c, "type=Vehicle&entityMap=true", &jsonld_link_value()).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED, "entityMap=true should return 201 CREATED");

    let entity_map_header = headers.get("ngsild-entitymap");
    assert!(entity_map_header.is_some(), "NGSILD-EntityMap header should be present");

    let map_id = entity_map_header.unwrap().to_str().unwrap();
    let resp = c
        .get(format!("{BASE_URL}/entityMaps/{}", encode(map_id)))
        .header("Accept", "application/json")
        .header("Link", jsonld_link_value())
        .send()
        .await
        .expect("get entity map failed");
    let map_body: Value = resp.json().await.unwrap_or_default();

    // Table 5.2.39-2: the entity ids of a map are the keys of its `entityMap`
    // index map, whose values name the sources holding each entity. The
    // representation has no `ids` member.
    let entity_map = map_body["entityMap"].as_object().expect("entityMap should be an object");
    assert!(
        entity_map.contains_key(local_id.as_str()),
        "EntityMap should contain local entity ID: {map_body}"
    );
    assert!(
        entity_map.contains_key(remote_id.as_str()),
        "EntityMap should contain remote (forwarded) entity ID: {map_body}"
    );

    delete_entity(&c, &local_id).await;
    delete_registration(&c, &reg_id).await;
    mock.shutdown().await;
}

/// `EntityMap` with local=true only contains local entity IDs.
#[tokio::test]
#[serial]
async fn distop_entity_map_local_true_excludes_remote() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let mock = MockServer::start().await;

    let local_id = unique_id("urn:ngsi-ld:Vehicle:map-local-only");
    let remote_id = unique_id("urn:ngsi-ld:Vehicle:map-remote-only");
    let reg_id = unique_id("urn:ngsi-ld:Registration:map-local");

    let entity = vehicle_entity(&local_id);
    let (status, _) = create_entity(&c, &entity).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let mock_entity = vehicle_entity(&remote_id);
    mock.set_stub_with_body(Method::GET, "/ngsi-ld/v1/entities", 200, &Value::Array(vec![mock_entity]))
        .await;

    let reg = vehicle_registration(&reg_id, None, &mock.url(""), Some("auxiliary"));
    let (status, _) = create_registration(&c, &reg).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let (status, headers, _) = query_entities(&c, "type=Vehicle&entityMap=true&local=true", &jsonld_link_value()).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let map_id = headers.get("ngsild-entitymap").unwrap().to_str().unwrap();
    let resp = c
        .get(format!("{BASE_URL}/entityMaps/{}", encode(map_id)))
        .header("Accept", "application/json")
        .header("Link", jsonld_link_value())
        .send()
        .await
        .expect("get entity map failed");
    let map_body: Value = resp.json().await.unwrap_or_default();

    // Table 5.2.39-2: entity ids are the keys of the `entityMap` index map.
    let entity_map = map_body["entityMap"].as_object().expect("entityMap should be an object");
    assert!(entity_map.contains_key(local_id.as_str()), "should contain local ID: {map_body}");
    assert!(
        !entity_map.contains_key(remote_id.as_str()),
        "should NOT contain remote ID when local=true: {map_body}"
    );

    delete_entity(&c, &local_id).await;
    delete_registration(&c, &reg_id).await;
    mock.shutdown().await;
}

/// splitEntities with q filter applies post-merge: only matching entities remain.
#[tokio::test]
#[serial]
async fn distop_entity_split_entities_q_filter_post_merge() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let mock = MockServer::start().await;

    let local_id = unique_id("urn:ngsi-ld:Vehicle:split-local");
    let remote_id = unique_id("urn:ngsi-ld:Vehicle:split-remote");
    let reg_id = unique_id("urn:ngsi-ld:Registration:split-q");

    let entity = vehicle_entity_with_speed(&local_id, 50);
    let (status, _) = create_entity(&c, &entity).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let mock_entity = vehicle_entity_with_speed(&remote_id, 80);
    mock.set_stub_with_body(Method::GET, "/ngsi-ld/v1/entities", 200, &Value::Array(vec![mock_entity]))
        .await;

    // Clause 5.7.2.4 merges the Entity Arrays of exclusive, redirect and
    // inclusive sources with the local result; an auxiliary source only
    // supplements entities already present, so an inclusive (default) mode is
    // what puts a remote-only entity into the aggregate the filters run over.
    let reg = vehicle_registration(&reg_id, None, &mock.url(""), None);
    let (status, _) = create_registration(&c, &reg).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let query = "type=Vehicle&splitEntities=true&q=speed>60";
    let (status, _, body) = query_entities(&c, query, &jsonld_link_value()).await;
    assert!(status.is_success(), "query should succeed, got {status}");

    let entities = body.as_array().expect("response should be array");
    let returned_ids: Vec<&str> = entities.iter().filter_map(|e| e["id"].as_str()).collect();

    assert!(
        !returned_ids.contains(&local_id.as_str()),
        "local entity (speed=50) should be filtered out by q=speed>60"
    );
    assert!(
        returned_ids.contains(&remote_id.as_str()),
        "remote entity (speed=80) should pass q=speed>60 filter"
    );

    delete_entity(&c, &local_id).await;
    delete_registration(&c, &reg_id).await;
    mock.shutdown().await;
}

/// splitEntities without filter returns all entities from both sources.
#[tokio::test]
#[serial]
async fn distop_entity_split_entities_returns_all_without_filter() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let mock = MockServer::start().await;

    let local_id = unique_id("urn:ngsi-ld:Vehicle:split-all-local");
    let remote_id = unique_id("urn:ngsi-ld:Vehicle:split-all-remote");
    let reg_id = unique_id("urn:ngsi-ld:Registration:split-all");

    let entity = vehicle_entity_with_speed(&local_id, 50);
    let (status, _) = create_entity(&c, &entity).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let mock_entity = vehicle_entity_with_speed(&remote_id, 80);
    mock.set_stub_with_body(Method::GET, "/ngsi-ld/v1/entities", 200, &Value::Array(vec![mock_entity]))
        .await;

    // Clause 5.7.2.4 merges the Entity Arrays of exclusive, redirect and
    // inclusive sources with the local result; an auxiliary source only
    // supplements entities already present, so an inclusive (default) mode is
    // what puts a remote-only entity into the aggregate the filters run over.
    let reg = vehicle_registration(&reg_id, None, &mock.url(""), None);
    let (status, _) = create_registration(&c, &reg).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let query = "type=Vehicle&splitEntities=true";
    let (status, _, body) = query_entities(&c, query, &jsonld_link_value()).await;
    assert!(status.is_success(), "query should succeed, got {status}");

    let entities = body.as_array().expect("response should be array");
    let returned_ids: Vec<&str> = entities.iter().filter_map(|e| e["id"].as_str()).collect();

    assert!(returned_ids.contains(&local_id.as_str()), "should contain local entity");
    assert!(returned_ids.contains(&remote_id.as_str()), "should contain remote entity");

    delete_entity(&c, &local_id).await;
    delete_registration(&c, &reg_id).await;
    mock.shutdown().await;
}

/// splitEntities with scopeQ filter applied post-merge.
#[tokio::test]
#[serial]
async fn distop_entity_split_entities_scope_filter_post_merge() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let mock = MockServer::start().await;

    let local_id = unique_id("urn:ngsi-ld:Vehicle:scope-local");
    let remote_id = unique_id("urn:ngsi-ld:Vehicle:scope-remote");
    let reg_id = unique_id("urn:ngsi-ld:Registration:scope-split");

    let entity = vehicle_entity_with_scope(&local_id, "/Madrid");
    let (status, _) = create_entity(&c, &entity).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let mock_entity = vehicle_entity_with_scope(&remote_id, "/Barcelona");
    mock.set_stub_with_body(Method::GET, "/ngsi-ld/v1/entities", 200, &Value::Array(vec![mock_entity]))
        .await;

    // Clause 5.7.2.4 merges the Entity Arrays of exclusive, redirect and
    // inclusive sources with the local result; an auxiliary source only
    // supplements entities already present, so an inclusive (default) mode is
    // what puts a remote-only entity into the aggregate the filters run over.
    let reg = vehicle_registration(&reg_id, None, &mock.url(""), None);
    let (status, _) = create_registration(&c, &reg).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let query = "type=Vehicle&splitEntities=true&scopeQ=/Barcelona";
    let (status, _, body) = query_entities(&c, query, &jsonld_link_value()).await;
    assert!(status.is_success(), "query should succeed, got {status}");

    let entities = body.as_array().expect("response should be array");
    let returned_ids: Vec<&str> = entities.iter().filter_map(|e| e["id"].as_str()).collect();

    assert!(
        !returned_ids.contains(&local_id.as_str()),
        "local entity (scope /Madrid) should be filtered out by scopeQ=/Barcelona"
    );
    assert!(
        returned_ids.contains(&remote_id.as_str()),
        "remote entity (scope /Barcelona) should pass scopeQ=/Barcelona filter"
    );

    delete_entity(&c, &local_id).await;
    delete_registration(&c, &reg_id).await;
    mock.shutdown().await;
}
