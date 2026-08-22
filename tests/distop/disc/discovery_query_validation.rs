//! Query-CSR (clause 5.10.2): JSON-LD and query-parameter validation, the
//! too-wide-query guard, pagination, and the JSON-LD array result shape. All
//! `T-LOCAL`.

use crate::{
    disc::{PEER_ENDPOINT, csr_discovery::matched_ids},
    oracle::{assert_problem_details, assert_status, json_body},
    registration::CsrBuilder,
    topology::{self, BrokerRole},
};
use andromeda_test_fixtures::{context::COMPOUND_CONTEXT, problem_detail::BAD_REQUEST_DATA};
use reqwest::Method;
use urlencoding::encode;

/// A syntactically malformed `Link` context header is rejected with `400
/// BadRequestData` (clauses 5.10.2, 6.3.5). Confirms REQ-DISC-17.
#[tokio::test]
async fn query_csr_applies_jsonld_validation() {
    let topology = topology::single().await.expect("single-broker topology");
    let response = topology
        .broker(BrokerRole::A)
        .client()
        .raw(Method::GET, "/ngsi-ld/v1/csourceRegistrations?type=Vehicle")
        .header("accept", "application/ld+json")
        .header("link", "this is not a valid link header")
        .send()
        .await
        .expect("query registrations with malformed link");
    assert_status(&response, 400);
    assert_problem_details(&response, BAD_REQUEST_DATA);
    topology.finish().await.expect("topology cleanup");
}

/// A query with no entity type, attribute list, `q`, or geo-query is too wide
/// and is rejected with `400 BadRequestData` (clause 5.10.2). Confirms
/// REQ-DISC-18, REQ-DISC-98.
#[tokio::test]
async fn query_csr_without_type_attrs_q_geoq_returns_400_too_wide() {
    let topology = topology::single().await.expect("single-broker topology");
    let response = topology
        .broker(BrokerRole::A)
        .client()
        .query_registrations("limit=5", None)
        .await
        .expect("too-wide query");
    assert_status(&response, 400);
    assert_problem_details(&response, BAD_REQUEST_DATA);
    topology.finish().await.expect("topology cleanup");
}

/// Entity ids alone, without a type, attribute list, `q`, or geo-query, are
/// still too wide and rejected with `400` (clause 5.10.2). Confirms
/// REQ-DISC-40.
#[tokio::test]
async fn query_csr_ids_only_without_type_attrs_q_geoq_returns_400() {
    let topology = topology::single().await.expect("single-broker topology");
    let id = topology.id("Vehicle:only");
    let response = topology
        .broker(BrokerRole::A)
        .client()
        .query_registrations(&format!("id={}", encode(&id)), None)
        .await
        .expect("ids-only query");
    assert_status(&response, 400);
    assert_problem_details(&response, BAD_REQUEST_DATA);
    topology.finish().await.expect("topology cleanup");
}

/// A non-URI entity id is rejected with `400 BadRequestData` (clause 5.10.2).
/// Confirms REQ-DISC-20.
#[tokio::test]
async fn query_csr_invalid_entity_id_returns_400() {
    let topology = topology::single().await.expect("single-broker topology");
    let response = topology
        .broker(BrokerRole::A)
        .client()
        .query_registrations("type=Vehicle&id=not-a-uri", Some(COMPOUND_CONTEXT.as_str()))
        .await
        .expect("invalid-id query");
    assert_status(&response, 400);
    assert_problem_details(&response, BAD_REQUEST_DATA);
    topology.finish().await.expect("topology cleanup");
}

/// A malformed `q`, geo-query, or temporal query each fail parsing and return
/// `400 BadRequestData` (clause 5.10.2). Confirms REQ-DISC-21.
#[tokio::test]
async fn query_csr_malformed_q_geoq_temporalq_return_400() {
    let topology = topology::single().await.expect("single-broker topology");
    let malformed = [
        "type=Vehicle&q=speed%3E%3E10",
        "type=Vehicle&geometry=Point&coordinates=%5B1%2C2%5D",
        "type=Vehicle&timerel=after",
    ];
    for query in malformed {
        let response = topology
            .broker(BrokerRole::A)
            .client()
            .query_registrations(query, Some(COMPOUND_CONTEXT.as_str()))
            .await
            .expect("malformed query");
        assert_status(&response, 400);
        assert_problem_details(&response, BAD_REQUEST_DATA);
    }
    topology.finish().await.expect("topology cleanup");
}

/// Pagination slices the result: with a unique registered type, `limit` bounds
/// the page and a full query returns every registration (clauses 5.10.2,
/// 5.5.9). Confirms REQ-DISC-39.
#[tokio::test]
async fn query_csr_pagination_limit_offset() {
    let topology = topology::single().await.expect("single-broker topology");
    let vtype = topology.id("PagedType");
    for index in 0..3 {
        let id = topology.id(&format!("registration:page:{index}"));
        let body = CsrBuilder::new(&id, PEER_ENDPOINT).entity_type(&vtype).build();
        let created = topology
            .broker(BrokerRole::A)
            .client()
            .create_registration(&body)
            .await
            .expect("create registration");
        assert_status(&created, 201);
    }
    let type_filter = format!("type={}", encode(&vtype));

    let full = topology
        .broker(BrokerRole::A)
        .client()
        .query_registrations(&type_filter, Some(COMPOUND_CONTEXT.as_str()))
        .await
        .expect("full query");
    assert_status(&full, 200);
    assert_eq!(
        matched_ids(&json_body(&full)).len(),
        3,
        "all three registrations of the unique type are returned"
    );

    let page = topology
        .broker(BrokerRole::A)
        .client()
        .query_registrations(&format!("{type_filter}&limit=2"), Some(COMPOUND_CONTEXT.as_str()))
        .await
        .expect("first page");
    assert_status(&page, 200);
    assert_eq!(matched_ids(&json_body(&page)).len(), 2, "limit=2 bounds the page to two registrations");
    topology.finish().await.expect("topology cleanup");
}

/// A matching Query-CSR returns a JSON-LD array of the matching registrations
/// (clauses 5.10.2, 5.2.9). Confirms REQ-DISC-42, REQ-DISC-93.
#[tokio::test]
async fn query_csr_returns_jsonld_array_of_matching_csrs() {
    let topology = topology::single().await.expect("single-broker topology");
    let vtype = topology.id("ArrayType");
    let id = topology.id("registration:array");
    let body = CsrBuilder::new(&id, PEER_ENDPOINT).entity_type(&vtype).build();
    let created = topology
        .broker(BrokerRole::A)
        .client()
        .create_registration(&body)
        .await
        .expect("create registration");
    assert_status(&created, 201);

    let response = topology
        .broker(BrokerRole::A)
        .client()
        .query_registrations(&format!("type={}", encode(&vtype)), Some(COMPOUND_CONTEXT.as_str()))
        .await
        .expect("query registrations");
    assert_status(&response, 200);
    let body = json_body(&response);
    assert!(body.is_array(), "the Query-CSR result is a JSON-LD array, got {body}");
    assert!(matched_ids(&body).contains(&id), "the matching registration is present in the array");
    topology.finish().await.expect("topology cleanup");
}
