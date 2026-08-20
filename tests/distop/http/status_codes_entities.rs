//! Response-code matrix for entity create, query and purge (clause 6.3.16,
//! 6.4.3): local success codes and `Location`, forwarded partial failure `207`
//! with a `BatchOperationResult`, invalid/absent/conflict codes, and the
//! single-source `501` on an unsupported representation.

use crate::{
    context_source_adapter::ContextSourceAdapter,
    entity::vehicle,
    http::forwarding::{FORWARDED_ENTITIES_PATH, context_link, provision_source, retrieval_source},
    oracle::{assert_problem_details, assert_status, json_body, location},
    topology::DistributedTopology,
};
use andromeda_test_client::response::TestResponse;
use andromeda_test_fixtures::problem_detail::{ALREADY_EXISTS, BAD_REQUEST_DATA, OPERATION_NOT_SUPPORTED};
use reqwest::Method;
use serde_json::json;

/// Whether the response body's JSON text mentions a substring, used to confirm a
/// `BatchOperationResult` carries a `registrationId` for the failed endpoint.
fn body_mentions(response: &TestResponse, needle: &str) -> bool {
    json_body(response).to_string().contains(needle)
}

/// A local create returns `201 Created` with a `Location` naming the new entity
/// (clause 6.4.3.1). Confirms REQ-HTTP-46.
#[tokio::test]
async fn dist_http_create_entity_local_returns_201_location() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let entity_id = topology.id("Vehicle:create-201");
    let response = topology
        .front()
        .client()
        .post_entity(&vehicle(&entity_id, 5))
        .with_ld_json()
        .send()
        .await
        .expect("local create");
    assert_status(&response, 201);
    assert!(location(&response).ends_with(&entity_id), "the Location names the created entity");
    topology.finish().await.expect("topology cleanup");
}

/// A forwarded create where one endpoint fails is a partial failure, mapped to
/// `207` with a `BatchOperationResult` naming the failed `registrationId`
/// (clause 6.3.16). Confirms REQ-HTTP-47.
#[tokio::test]
async fn dist_http_create_entity_forwarded_partial_failure_returns_207_batch_result() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let entity_id = topology.id("Vehicle:create-207");
    let reg_id = topology.id("registration:create-207");
    let mock = ContextSourceAdapter::start().await;

    assert_status(
        &topology
            .front()
            .client()
            .create_registration(&provision_source(&reg_id, &mock.url(""), &entity_id, "inclusive", &["createEntity"]))
            .await
            .expect("register source"),
        201,
    );
    mock.set_stub(Method::POST, FORWARDED_ENTITIES_PATH, 500).await;

    let response = topology
        .front()
        .client()
        .post_entity(&vehicle(&entity_id, 5))
        .with_ld_json()
        .send()
        .await
        .expect("forwarded create");
    assert_status(&response, 207);
    assert!(
        body_mentions(&response, "registrationId"),
        "the 207 BatchOperationResult names the failed registrationId"
    );

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A create with an invalid body is rejected with `400 BadRequestData`
/// (clause 6.4.3.1). Confirms REQ-HTTP-48.
#[tokio::test]
async fn dist_http_create_entity_invalid_body_returns_400_problem_details() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let entity_id = topology.id("Vehicle:create-400");
    let response = topology
        .front()
        .client()
        .post_entity(&json!({ "id": entity_id, "@context": context_link() }))
        .with_ld_json()
        .send()
        .await
        .expect("invalid create");
    assert_status(&response, 400);
    assert_problem_details(&response, BAD_REQUEST_DATA);
    topology.finish().await.expect("topology cleanup");
}

/// Re-creating an existing local entity is rejected with `409 AlreadyExists`
/// (clause 6.4.3.1). Confirms REQ-HTTP-49.
#[tokio::test]
async fn dist_http_create_entity_already_exists_local_returns_409() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let entity_id = topology.id("Vehicle:create-409");
    assert_status(
        &topology
            .front()
            .client()
            .post_entity(&vehicle(&entity_id, 5))
            .with_ld_json()
            .send()
            .await
            .expect("first create"),
        201,
    );

    let response = topology
        .front()
        .client()
        .post_entity(&vehicle(&entity_id, 5))
        .with_ld_json()
        .send()
        .await
        .expect("duplicate create");
    assert_status(&response, 409);
    assert_problem_details(&response, ALREADY_EXISTS);
    topology.finish().await.expect("topology cleanup");
}

/// Creating an entity whose id is owned by an exclusive registration is a
/// conflict, mapped to `409` (clause 5.7.2). Confirms REQ-HTTP-49.
#[tokio::test]
async fn dist_http_create_entity_exclusive_registration_owns_id_returns_409() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let entity_id = topology.id("Vehicle:create-owned");
    let reg_id = topology.id("registration:create-owned");
    let mock = ContextSourceAdapter::start().await;

    assert_status(
        &topology
            .front()
            .client()
            .create_registration(&provision_source(&reg_id, &mock.url(""), &entity_id, "exclusive", &["createEntity"]))
            .await
            .expect("register exclusive source"),
        201,
    );

    let response = topology
        .front()
        .client()
        .post_entity(&vehicle(&entity_id, 5))
        .with_ld_json()
        .send()
        .await
        .expect("create owned id");
    assert_status(&response, 409);

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A create for which no matching source advertises the operation is rejected
/// with `422 OperationNotSupported` (clause 6.3.16). Confirms REQ-HTTP-50.
#[tokio::test]
async fn dist_http_create_entity_operation_unavailable_returns_422() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let entity_id = topology.id("Vehicle:create-422");
    let reg_id = topology.id("registration:create-422");
    let mock = ContextSourceAdapter::start().await;

    assert_status(
        &topology
            .front()
            .client()
            .create_registration(&provision_source(&reg_id, &mock.url(""), &entity_id, "exclusive", &["retrieveEntity"]))
            .await
            .expect("register read-only source"),
        201,
    );

    let response = topology
        .front()
        .client()
        .post_entity(&vehicle(&entity_id, 5))
        .with_ld_json()
        .send()
        .await
        .expect("create with unavailable operation");
    assert_status(&response, 422);
    assert_problem_details(&response, OPERATION_NOT_SUPPORTED);

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// An invalid query is rejected with `400 BadRequestData` (clause 6.4.3.2).
/// Confirms REQ-HTTP-57.
#[tokio::test]
async fn dist_http_query_entities_invalid_returns_400_problem_details() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let response = topology
        .front()
        .client()
        .query_entities("type=Vehicle&q=speed%3E", "application/ld+json", Some(context_link()))
        .await
        .expect("invalid query");
    assert_status(&response, 400);
    assert_problem_details(&response, BAD_REQUEST_DATA);
    topology.finish().await.expect("topology cleanup");
}

/// A query resolving to a single source that cannot serve the representation maps
/// to `501` (clause 6.3.16). Confirms REQ-HTTP-58.
#[tokio::test]
async fn dist_http_query_entities_source_unsupported_format_returns_501() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let entity_id = topology.id("Vehicle:query-501");
    let reg_id = topology.id("registration:query-501");
    let mock = ContextSourceAdapter::start().await;

    assert_status(
        &topology
            .front()
            .client()
            .create_registration(&retrieval_source(&reg_id, &mock.url(""), &entity_id, "exclusive"))
            .await
            .expect("register source"),
        201,
    );
    mock.set_stub(Method::GET, FORWARDED_ENTITIES_PATH, 501).await;

    let response = topology
        .front()
        .client()
        .query_entities("type=Vehicle", "application/ld+json", Some(context_link()))
        .await
        .expect("forwarded query");
    assert_status(&response, 501);

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A local purge returns `204 No Content` (clause 6.4.3.4). Confirms REQ-HTTP-61.
#[tokio::test]
async fn dist_http_purge_entities_local_returns_204() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let entity_id = topology.id("Vehicle:purge-204");
    assert_status(
        &topology
            .front()
            .client()
            .post_entity(&vehicle(&entity_id, 5))
            .with_ld_json()
            .send()
            .await
            .expect("local create"),
        201,
    );

    let query = format!("type=Vehicle&idPattern={}", urlencoding::encode(topology.id_prefix()));
    let response = topology
        .front()
        .client()
        .delete_entities_by_query(&query, Some(context_link()))
        .await
        .expect("local purge");
    assert_status(&response, 204);
    topology.finish().await.expect("topology cleanup");
}

/// A purge where a forwarded endpoint fails is a partial failure, mapped to `207`
/// (clause 6.3.16). Confirms REQ-HTTP-61.
#[tokio::test]
async fn dist_http_purge_entities_partial_failure_returns_207_batch_result() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let entity_id = topology.id("Vehicle:purge-207");
    let reg_id = topology.id("registration:purge-207");
    let mock = ContextSourceAdapter::start().await;

    assert_status(
        &topology
            .front()
            .client()
            .post_entity(&vehicle(&entity_id, 5))
            .with_ld_json()
            .send()
            .await
            .expect("local create"),
        201,
    );
    assert_status(
        &topology
            .front()
            .client()
            .create_registration(&provision_source(&reg_id, &mock.url(""), &entity_id, "inclusive", &["deleteEntity"]))
            .await
            .expect("register source"),
        201,
    );
    mock.set_stub(Method::DELETE, FORWARDED_ENTITIES_PATH, 500).await;

    let query = format!("type=Vehicle&idPattern={}&local=false", urlencoding::encode(topology.id_prefix()));
    let response = topology
        .front()
        .client()
        .delete_entities_by_query(&query, Some(context_link()))
        .await
        .expect("forwarded purge");
    assert_status(&response, 207);

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// An invalid purge query is rejected with `400` (clause 6.4.3.4).
/// Confirms REQ-HTTP-61.
#[tokio::test]
async fn dist_http_purge_entities_invalid_returns_400() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let response = topology
        .front()
        .client()
        .delete_entities_by_query("q=speed%3E", Some(context_link()))
        .await
        .expect("invalid purge");
    assert_status(&response, 400);
    topology.finish().await.expect("topology cleanup");
}

/// Retrieving an entity restricted to attributes it does not hold returns `404`
/// (clause 6.4.3.2). Confirms REQ-HTTP-63.
#[tokio::test]
async fn dist_http_retrieve_entity_attrs_absent_returns_404() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let entity_id = topology.id("Vehicle:retrieve-attrs-404");
    assert_status(
        &topology
            .front()
            .client()
            .post_entity(&vehicle(&entity_id, 5))
            .with_ld_json()
            .send()
            .await
            .expect("local create"),
        201,
    );

    let response = topology
        .front()
        .client()
        .get_entity(&entity_id, "attrs=color", "application/ld+json", Some(context_link()))
        .await
        .expect("retrieve absent attrs");
    assert_status(&response, 404);
    topology.finish().await.expect("topology cleanup");
}
