//! Registry API response codes for `csourceRegistrations` (clause 6.8, 6.9): the
//! `201`/`Location` create, the create `400`/`409`/`422` rejections, the array
//! query, and the retrieve/update/delete `200`/`204`/`400`/`404` outcomes. All
//! `T-LOCAL` (Registry-API operations are always local).

use crate::{
    http::forwarding::{PLACEHOLDER_ENDPOINT, context_link},
    oracle::{assert_problem_details, assert_status, json_body, location},
    registration::{CsrBuilder, csr_basic},
    topology::{self, BrokerRole},
};
use andromeda_test_fixtures::problem_detail::{ALREADY_EXISTS, BAD_REQUEST_DATA};
use serde_json::json;

/// A valid registration create returns `201` with a `Location` naming the CSR
/// (clause 6.8.3.1). Confirms REQ-HTTP-79.
#[tokio::test]
async fn dist_http_create_csource_registration_returns_201_location() {
    let topology = topology::single().await.expect("single-broker topology");
    let reg_id = topology.id("registration:reg-201");
    let response = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .create_registration(&csr_basic(&reg_id, PLACEHOLDER_ENDPOINT))
        .await
        .expect("create registration");
    assert_status(&response, 201);
    assert!(location(&response).ends_with(&reg_id), "the Location names the created registration");
    topology.finish().await.expect("topology cleanup");
}

/// An invalid registration body is rejected with `400 BadRequestData`
/// (clause 6.8.3.1). Confirms REQ-HTTP-80.
#[tokio::test]
async fn dist_http_create_csource_registration_invalid_returns_400() {
    let topology = topology::single().await.expect("single-broker topology");
    let response = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .create_registration(&json!({ "type": "ContextSourceRegistration", "@context": context_link() }))
        .await
        .expect("invalid registration create");
    assert_status(&response, 400);
    assert_problem_details(&response, BAD_REQUEST_DATA);
    topology.finish().await.expect("topology cleanup");
}

/// Re-creating an existing registration is rejected with `409 AlreadyExists`
/// (clause 6.8.3.1). Confirms REQ-HTTP-80.
#[tokio::test]
async fn dist_http_create_csource_registration_duplicate_returns_409() {
    let topology = topology::single().await.expect("single-broker topology");
    let reg_id = topology.id("registration:reg-409");
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .create_registration(&csr_basic(&reg_id, PLACEHOLDER_ENDPOINT))
            .await
            .expect("first create"),
        201,
    );

    let response = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .create_registration(&csr_basic(&reg_id, PLACEHOLDER_ENDPOINT))
        .await
        .expect("duplicate create");
    assert_status(&response, 409);
    assert_problem_details(&response, ALREADY_EXISTS);
    topology.finish().await.expect("topology cleanup");
}

/// An auxiliary registration declaring write operations violates clause
/// 5.9.2.4, which names the error type `BadRequestData`, so the HTTP binding
/// answers `400` (clause 6.8.3.1). Confirms REQ-HTTP-80.
#[tokio::test]
async fn dist_http_create_csource_registration_auxiliary_write_operations_returns_400() {
    let topology = topology::single().await.expect("single-broker topology");
    let reg_id = topology.id("registration:reg-422");
    let entity_id = topology.id("Vehicle:reg-422");
    let registration = CsrBuilder::new(&reg_id, PLACEHOLDER_ENDPOINT)
        .mode("auxiliary")
        .entity_id(&entity_id)
        .operations(&["createEntity", "updateEntity", "deleteEntity"])
        .build();
    let response = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .create_registration(&registration)
        .await
        .expect("auxiliary registration with write operations");
    assert_status(&response, 400);
    topology.finish().await.expect("topology cleanup");
}

/// A registration query returns `200` with an array body (clause 6.9.3.2). The
/// query names a type: clause 5.10.2.4 rejects a discovery query that carries
/// none of type, attrs, q or georel as too wide. Confirms REQ-HTTP-82.
#[tokio::test]
async fn dist_http_query_csource_registrations_returns_200_array() {
    let topology = topology::single().await.expect("single-broker topology");
    let reg_id = topology.id("registration:reg-array");
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .create_registration(&csr_basic(&reg_id, PLACEHOLDER_ENDPOINT))
            .await
            .expect("create registration"),
        201,
    );

    let response = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .query_registrations("type=Vehicle", None)
        .await
        .expect("registration query");
    assert_status(&response, 200);
    assert!(json_body(&response).is_array(), "a registration query returns an array");
    topology.finish().await.expect("topology cleanup");
}

/// Registration retrieve returns `200` for an existing id, `400` for an invalid
/// id, and `404` for an unknown id (clause 6.9.3.1). Confirms REQ-HTTP-83.
#[tokio::test]
async fn dist_http_retrieve_csource_registration_returns_200_400_404() {
    let topology = topology::single().await.expect("single-broker topology");
    let reg_id = topology.id("registration:reg-retrieve");
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .create_registration(&csr_basic(&reg_id, PLACEHOLDER_ENDPOINT))
            .await
            .expect("create registration"),
        201,
    );

    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .retrieve_registration(&reg_id, None)
            .await
            .expect("retrieve existing"),
        200,
    );
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .retrieve_registration("not a uri", None)
            .await
            .expect("retrieve invalid"),
        400,
    );
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .retrieve_registration(&topology.id("registration:reg-unknown"), None)
            .await
            .expect("retrieve unknown"),
        404,
    );
    topology.finish().await.expect("topology cleanup");
}

/// Registration update returns `204` for a valid patch, `400` for an invalid
/// patch, and `404` for an unknown id (clause 6.9.3.3). Confirms REQ-HTTP-84.
#[tokio::test]
async fn dist_http_update_csource_registration_returns_204_400_404() {
    let topology = topology::single().await.expect("single-broker topology");
    let reg_id = topology.id("registration:reg-update");
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .create_registration(&csr_basic(&reg_id, PLACEHOLDER_ENDPOINT))
            .await
            .expect("create registration"),
        201,
    );

    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .update_registration(&reg_id, &json!({ "description": "updated" }), Some(context_link()))
            .await
            .expect("valid update"),
        204,
    );
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .update_registration(&reg_id, &json!({ "mode": "not-a-mode" }), Some(context_link()))
            .await
            .expect("invalid update"),
        400,
    );
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .update_registration(
                &topology.id("registration:reg-update-unknown"),
                &json!({ "description": "x" }),
                Some(context_link()),
            )
            .await
            .expect("unknown update"),
        404,
    );
    topology.finish().await.expect("topology cleanup");
}

/// Registration delete returns `204` for an existing id, `400` for an invalid id,
/// and `404` for an unknown id (clause 6.9.3.4). Confirms REQ-HTTP-85.
#[tokio::test]
async fn dist_http_delete_csource_registration_returns_204_400_404() {
    let topology = topology::single().await.expect("single-broker topology");
    let reg_id = topology.id("registration:reg-delete");
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .create_registration(&csr_basic(&reg_id, PLACEHOLDER_ENDPOINT))
            .await
            .expect("create registration"),
        201,
    );

    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .delete_registration("not a uri")
            .await
            .expect("delete invalid"),
        400,
    );
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .delete_registration(&topology.id("registration:reg-delete-unknown"))
            .await
            .expect("delete unknown"),
        404,
    );
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .delete_registration(&reg_id)
            .await
            .expect("delete existing"),
        204,
    );
    topology.finish().await.expect("topology cleanup");
}
