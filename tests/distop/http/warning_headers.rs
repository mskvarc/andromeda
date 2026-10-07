//! `NGSILD-Warning` fault-injection matrix (clause 6.3.11): a forwarded query to a mock Context Source that returns an error, an invalid payload, or is unreachable must surface the correct warning code (110/111/199/299) while the query itself still succeeds with `200 OK`.

use crate::{
    context_source_adapter::ContextSourceAdapter,
    http::forwarding::{FORWARDED_ENTITIES_PATH, context_link, retrieval_source},
    oracle::{assert_status, has_warning, json_body, warning_codes},
    registration::CsrBuilder,
    topology::{self, BrokerRole},
};
use pretty_assertions::assert_eq;
use reqwest::Method;
use serde_json::json;

/// A second query inside the registration's `cacheDuration` reuses the cached source response and carries warning `110` (clause 6.3.17: "where a Context Source Registration matches the request and a previous forwarded response has been cached and a subsequent request occurs before the Context Source Registration cacheDuration (as defined in Table 5.2.34-1) has been reached, the result may incorporate data cached from a previous response. To indicate that data from a cache has been included in the response, the HTTP header "NGSILD-Warning" shall be included. The value shall match the IANA Warning Code [32] 110 - Response is Stale"). The registration therefore sets `management.cacheDuration`, the source answers a valid empty array so there is a response to cache, and the warning is asserted on the second query only.
#[tokio::test]
async fn dist_http_query_cached_result_returns_warning_110() {
    let topology = topology::single().await.expect("single-broker topology");
    let vtype = topology.id("Vehicle");
    let entity_id = topology.id("Vehicle:warn-110");
    let reg_id = topology.id("registration:warn-110");
    let mock = ContextSourceAdapter::start().await;

    let registration = CsrBuilder::new(&reg_id, &mock.url(""))
        .mode("inclusive")
        .entity_type(&vtype)
        .entity_id(&entity_id)
        .property_names(&["speed"])
        .operations(&["retrieveEntity", "queryEntity", "queryBatch"])
        .management(json!({ "cacheDuration": "PT10S" }))
        .build();
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .create_registration(&registration)
            .await
            .expect("register mock source"),
        201,
    );
    mock.set_stub_with_body(Method::GET, FORWARDED_ENTITIES_PATH, 200, &json!([])).await;

    let first = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .query_entities(&format!("type={}", urlencoding::encode(&vtype)), "application/ld+json", Some(context_link()))
        .await
        .expect("first forwarded query");
    assert_status(&first, 200);
    assert!(
        !has_warning(&first, 110),
        "the first query contacts the source and carries no stale warning, got {:?}",
        warning_codes(&first)
    );

    let response = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .query_entities(&format!("type={}", urlencoding::encode(&vtype)), "application/ld+json", Some(context_link()))
        .await
        .expect("second forwarded query");
    assert_status(&response, 200);
    assert!(
        has_warning(&response, 110),
        "a result served from the cache within cacheDuration must carry warning 110, got {:?}",
        warning_codes(&response)
    );
    assert_eq!(mock.get_total_count(), 1, "the second query is served from the cache");

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A source returning `500` is abnormal, so the forwarded query carries a `NGSILD-Warning` (clause 6.3.11).
#[tokio::test]
async fn dist_http_query_abnormal_source_sets_warning_header() {
    let topology = topology::single().await.expect("single-broker topology");
    let vtype = topology.id("Vehicle");
    let entity_id = topology.id("Vehicle:warn-abnormal");
    let reg_id = topology.id("registration:warn-abnormal");
    let mock = ContextSourceAdapter::start().await;

    let registration = retrieval_source(&reg_id, &mock.url(""), &entity_id, &vtype, "inclusive");
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .create_registration(&registration)
            .await
            .expect("register mock source"),
        201,
    );
    mock.set_stub(Method::GET, FORWARDED_ENTITIES_PATH, 500).await;

    let response = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .query_entities(&format!("type={}", urlencoding::encode(&vtype)), "application/ld+json", Some(context_link()))
        .await
        .expect("forwarded query");
    assert_status(&response, 200);
    assert!(!warning_codes(&response).is_empty(), "an abnormal source must set a warning header");

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A source that answers within the timeout with an undecodable payload yields warning `111` (clause 6.3.11).
#[tokio::test]
async fn dist_http_query_invalid_payload_within_timeout_returns_warning_111() {
    let topology = topology::single().await.expect("single-broker topology");
    let vtype = topology.id("Vehicle");
    let entity_id = topology.id("Vehicle:warn-111");
    let reg_id = topology.id("registration:warn-111");
    let mock = ContextSourceAdapter::start().await;

    let registration = retrieval_source(&reg_id, &mock.url(""), &entity_id, &vtype, "inclusive");
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .create_registration(&registration)
            .await
            .expect("register mock source"),
        201,
    );
    mock.set_stub_with_raw_body(Method::GET, FORWARDED_ENTITIES_PATH, 200, "this is not json", "text/plain")
        .await;

    let response = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .query_entities(&format!("type={}", urlencoding::encode(&vtype)), "application/ld+json", Some(context_link()))
        .await
        .expect("forwarded query");
    assert_status(&response, 200);
    assert!(
        has_warning(&response, 111),
        "an undecodable payload must yield warning 111, got {:?}",
        warning_codes(&response)
    );

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A source that never responds (unreachable) is a timeout, yielding warning `199` (clause 6.3.11).
#[tokio::test]
async fn dist_http_query_source_timeout_returns_warning_199() {
    let topology = topology::single().await.expect("single-broker topology");
    let vtype = topology.id("Vehicle");
    let entity_id = topology.id("Vehicle:warn-199");
    let reg_id = topology.id("registration:warn-199");
    let mock = ContextSourceAdapter::start().await;
    let endpoint = mock.url("");
    // Bind then release the port so the registered endpoint is unreachable, which
    // the broker classifies as a timeout rather than a definite error response.
    mock.shutdown().await;

    let registration = retrieval_source(&reg_id, &endpoint, &entity_id, &vtype, "inclusive");
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .create_registration(&registration)
            .await
            .expect("register mock source"),
        201,
    );

    let response = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .query_entities(&format!("type={}", urlencoding::encode(&vtype)), "application/ld+json", Some(context_link()))
        .await
        .expect("forwarded query");
    assert_status(&response, 200);
    assert!(
        has_warning(&response, 199),
        "an unreachable source must yield warning 199, got {:?}",
        warning_codes(&response)
    );

    topology.finish().await.expect("topology cleanup");
}

/// A source that answers promptly with `403` is a definite error response within the timeout, yielding warning `299` (clause 6.3.11).
#[tokio::test]
async fn dist_http_query_source_error_within_timeout_returns_warning_299() {
    let topology = topology::single().await.expect("single-broker topology");
    let vtype = topology.id("Vehicle");
    let entity_id = topology.id("Vehicle:warn-299");
    let reg_id = topology.id("registration:warn-299");
    let mock = ContextSourceAdapter::start().await;

    let registration = retrieval_source(&reg_id, &mock.url(""), &entity_id, &vtype, "inclusive");
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .create_registration(&registration)
            .await
            .expect("register mock source"),
        201,
    );
    mock.set_stub(Method::GET, FORWARDED_ENTITIES_PATH, 403).await;

    let response = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .query_entities(&format!("type={}", urlencoding::encode(&vtype)), "application/ld+json", Some(context_link()))
        .await
        .expect("forwarded query");
    assert_status(&response, 200);
    assert!(
        has_warning(&response, 299),
        "a prompt error response must yield warning 299, got {:?}",
        warning_codes(&response)
    );

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A non-NGSI-LD payload is rejected (warning `111`) and contributes no entities to the merged result (clause 6.3.11).
#[tokio::test]
async fn dist_http_query_non_ngsi_payload_rejected_and_excluded() {
    let topology = topology::single().await.expect("single-broker topology");
    let vtype = topology.id("Vehicle");
    let entity_id = topology.id("Vehicle:warn-excluded");
    let reg_id = topology.id("registration:warn-excluded");
    let mock = ContextSourceAdapter::start().await;

    let registration = retrieval_source(&reg_id, &mock.url(""), &entity_id, &vtype, "inclusive");
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .create_registration(&registration)
            .await
            .expect("register mock source"),
        201,
    );
    mock.set_stub_with_raw_body(Method::GET, FORWARDED_ENTITIES_PATH, 200, "<html>not ngsi-ld</html>", "text/html")
        .await;

    let response = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .query_entities(&format!("type={}", urlencoding::encode(&vtype)), "application/ld+json", Some(context_link()))
        .await
        .expect("forwarded query");
    assert_status(&response, 200);
    assert!(
        has_warning(&response, 111),
        "a non-NGSI-LD payload must yield warning 111, got {:?}",
        warning_codes(&response)
    );
    let body = json_body(&response);
    let results = body.as_array().unwrap_or_else(|| panic!("entity query returns an array"));
    assert!(results.is_empty(), "the rejected source contributes no entities, got {results:?}");

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A source returning `404` reports absence of data, not an error, so no warning is flagged (clause 6.3.11).
#[tokio::test]
async fn dist_http_query_source_404_no_data_not_flagged() {
    let topology = topology::single().await.expect("single-broker topology");
    let vtype = topology.id("Vehicle");
    let entity_id = topology.id("Vehicle:warn-404");
    let reg_id = topology.id("registration:warn-404");
    let mock = ContextSourceAdapter::start().await;

    let registration = retrieval_source(&reg_id, &mock.url(""), &entity_id, &vtype, "inclusive");
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .create_registration(&registration)
            .await
            .expect("register mock source"),
        201,
    );
    mock.set_stub(Method::GET, FORWARDED_ENTITIES_PATH, 404).await;

    let response = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .query_entities(&format!("type={}", urlencoding::encode(&vtype)), "application/ld+json", Some(context_link()))
        .await
        .expect("forwarded query");
    assert_status(&response, 200);
    assert!(
        warning_codes(&response).is_empty(),
        "a 404 source must not flag a warning, got {:?}",
        warning_codes(&response)
    );

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}
