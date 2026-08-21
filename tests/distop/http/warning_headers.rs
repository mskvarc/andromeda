//! `NGSILD-Warning` fault-injection matrix (clause 6.3.11): a forwarded query to
//! a mock Context Source that returns an error, an invalid payload, or is
//! unreachable must surface the correct warning code (110/111/199/299) while the
//! query itself still succeeds with `200 OK`. All `T-MOCK`.

use crate::{
    context_source_adapter::ContextSourceAdapter,
    http::forwarding::{FORWARDED_ENTITIES_PATH, context_link, retrieval_source},
    oracle::{assert_status, has_warning, json_body, warning_codes},
    topology::DistributedTopology,
};
use reqwest::Method;

/// A stale cached result carries warning `110`. Caching and `cacheDuration`
/// enforcement is not implemented, so this is a tracked gap. Confirms REQ-HTTP-16,
/// REQ-HTTP-17, REQ-HTTP-19.
#[tokio::test]
async fn dist_http_query_cached_result_returns_warning_110() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let entity_id = topology.id("Vehicle:warn-110");
    let reg_id = topology.id("registration:warn-110");
    let mock = ContextSourceAdapter::start().await;

    let registration = retrieval_source(&reg_id, &mock.url(""), &entity_id, "inclusive");
    assert_status(
        &topology
            .front()
            .client()
            .create_registration(&registration)
            .await
            .expect("register mock source"),
        201,
    );
    mock.set_stub(Method::GET, FORWARDED_ENTITIES_PATH, 200).await;

    let response = topology
        .front()
        .client()
        .query_entities("type=Vehicle", "application/ld+json", Some(context_link()))
        .await
        .expect("forwarded query");
    assert_status(&response, 200);
    assert!(
        has_warning(&response, 110),
        "a stale cached result must carry warning 110, got {:?}",
        warning_codes(&response)
    );

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A source returning `500` is abnormal, so the forwarded query carries a
/// `NGSILD-Warning` (clause 6.3.11). Confirms REQ-HTTP-18.
#[tokio::test]
async fn dist_http_query_abnormal_source_sets_warning_header() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let entity_id = topology.id("Vehicle:warn-abnormal");
    let reg_id = topology.id("registration:warn-abnormal");
    let mock = ContextSourceAdapter::start().await;

    let registration = retrieval_source(&reg_id, &mock.url(""), &entity_id, "inclusive");
    assert_status(
        &topology
            .front()
            .client()
            .create_registration(&registration)
            .await
            .expect("register mock source"),
        201,
    );
    mock.set_stub(Method::GET, FORWARDED_ENTITIES_PATH, 500).await;

    let response = topology
        .front()
        .client()
        .query_entities("type=Vehicle", "application/ld+json", Some(context_link()))
        .await
        .expect("forwarded query");
    assert_status(&response, 200);
    assert!(!warning_codes(&response).is_empty(), "an abnormal source must set a warning header");

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A source that answers within the timeout with an undecodable payload yields
/// warning `111` (clause 6.3.11). Confirms REQ-HTTP-20.
#[tokio::test]
async fn dist_http_query_invalid_payload_within_timeout_returns_warning_111() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let entity_id = topology.id("Vehicle:warn-111");
    let reg_id = topology.id("registration:warn-111");
    let mock = ContextSourceAdapter::start().await;

    let registration = retrieval_source(&reg_id, &mock.url(""), &entity_id, "inclusive");
    assert_status(
        &topology
            .front()
            .client()
            .create_registration(&registration)
            .await
            .expect("register mock source"),
        201,
    );
    mock.set_stub_with_raw_body(Method::GET, FORWARDED_ENTITIES_PATH, 200, "this is not json", "text/plain")
        .await;

    let response = topology
        .front()
        .client()
        .query_entities("type=Vehicle", "application/ld+json", Some(context_link()))
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

/// A source that never responds (unreachable) is a timeout, yielding warning
/// `199` (clause 6.3.11). Confirms REQ-HTTP-21.
#[tokio::test]
async fn dist_http_query_source_timeout_returns_warning_199() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let entity_id = topology.id("Vehicle:warn-199");
    let reg_id = topology.id("registration:warn-199");
    let mock = ContextSourceAdapter::start().await;
    let endpoint = mock.url("");
    // Bind then release the port so the registered endpoint is unreachable, which
    // the broker classifies as a timeout rather than a definite error response.
    mock.shutdown().await;

    let registration = retrieval_source(&reg_id, &endpoint, &entity_id, "inclusive");
    assert_status(
        &topology
            .front()
            .client()
            .create_registration(&registration)
            .await
            .expect("register mock source"),
        201,
    );

    let response = topology
        .front()
        .client()
        .query_entities("type=Vehicle", "application/ld+json", Some(context_link()))
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

/// A source that answers promptly with `403` is a definite error response within
/// the timeout, yielding warning `299` (clause 6.3.11). Confirms REQ-HTTP-22.
#[tokio::test]
async fn dist_http_query_source_error_within_timeout_returns_warning_299() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let entity_id = topology.id("Vehicle:warn-299");
    let reg_id = topology.id("registration:warn-299");
    let mock = ContextSourceAdapter::start().await;

    let registration = retrieval_source(&reg_id, &mock.url(""), &entity_id, "inclusive");
    assert_status(
        &topology
            .front()
            .client()
            .create_registration(&registration)
            .await
            .expect("register mock source"),
        201,
    );
    mock.set_stub(Method::GET, FORWARDED_ENTITIES_PATH, 403).await;

    let response = topology
        .front()
        .client()
        .query_entities("type=Vehicle", "application/ld+json", Some(context_link()))
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

/// A non-NGSI-LD payload is rejected (warning `111`) and contributes no entities
/// to the merged result (clause 6.3.11). Confirms REQ-HTTP-23.
#[tokio::test]
async fn dist_http_query_non_ngsi_payload_rejected_and_excluded() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let entity_id = topology.id("Vehicle:warn-excluded");
    let reg_id = topology.id("registration:warn-excluded");
    let mock = ContextSourceAdapter::start().await;

    let registration = retrieval_source(&reg_id, &mock.url(""), &entity_id, "inclusive");
    assert_status(
        &topology
            .front()
            .client()
            .create_registration(&registration)
            .await
            .expect("register mock source"),
        201,
    );
    mock.set_stub_with_raw_body(Method::GET, FORWARDED_ENTITIES_PATH, 200, "<html>not ngsi-ld</html>", "text/html")
        .await;

    let response = topology
        .front()
        .client()
        .query_entities("type=Vehicle", "application/ld+json", Some(context_link()))
        .await
        .expect("forwarded query");
    assert_status(&response, 200);
    assert!(
        has_warning(&response, 111),
        "a non-NGSI-LD payload must yield warning 111, got {:?}",
        warning_codes(&response)
    );
    let body = json_body(&response);
    let results = body.as_array().unwrap_or_else(|| unreachable!("entity query returns an array"));
    assert!(results.is_empty(), "the rejected source contributes no entities, got {results:?}");

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A source returning `404` reports absence of data, not an error, so no warning
/// is flagged (clause 6.3.11). Confirms REQ-HTTP-24.
#[tokio::test]
async fn dist_http_query_source_404_no_data_not_flagged() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let entity_id = topology.id("Vehicle:warn-404");
    let reg_id = topology.id("registration:warn-404");
    let mock = ContextSourceAdapter::start().await;

    let registration = retrieval_source(&reg_id, &mock.url(""), &entity_id, "inclusive");
    assert_status(
        &topology
            .front()
            .client()
            .create_registration(&registration)
            .await
            .expect("register mock source"),
        201,
    );
    mock.set_stub(Method::GET, FORWARDED_ENTITIES_PATH, 404).await;

    let response = topology
        .front()
        .client()
        .query_entities("type=Vehicle", "application/ld+json", Some(context_link()))
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
