//! Single-source and multi-source error mapping (clause 6.3.16 / 6.3.18): when a
//! distributed operation resolves to exactly one authoritative source, that
//! source's outcome maps onto the client status (508 loop, 504 timeout, 404
//! absent, 502 other), while a multi-endpoint operation with a failing member
//! yields `207`. Auxiliary registrations never receive unsafe methods.

use crate::{
    context_source_adapter::ContextSourceAdapter,
    entity::vehicle,
    http::forwarding::{FORWARDED_ENTITIES_PATH, context_link, forwarded_entity_path, provision_source, retrieval_source},
    oracle::{assert_status, json_body},
    topology::{BrokerRole, DistributedTopology},
};
use reqwest::Method;

/// A single source that forwards back to the origin broker is a loop, mapped to
/// `508 Loop Detected` (clause 6.3.18). Confirms REQ-HTTP-25.
#[tokio::test]
async fn dist_http_single_source_loop_back_returns_508() {
    let topology = DistributedTopology::pair().await.expect("two-broker topology");
    let front = topology.front();
    let peer = topology.broker(BrokerRole::B);
    let entity_id = topology.id("Vehicle:loop");
    let front_reg = topology.id("registration:loop-front");
    let peer_reg = topology.id("registration:loop-peer");

    let to_peer = retrieval_source(&front_reg, peer.peer_url(), &entity_id, "exclusive");
    assert_status(&front.client().create_registration(&to_peer).await.expect("front registers peer"), 201);
    let back_to_front = retrieval_source(&peer_reg, front.peer_url(), &entity_id, "exclusive");
    assert_status(&peer.client().create_registration(&back_to_front).await.expect("peer registers front"), 201);

    let response = front
        .client()
        .get_entity(&entity_id, "", "application/ld+json", Some(context_link()))
        .await
        .expect("looping retrieve");
    assert_status(&response, 508);

    topology.finish().await.expect("topology cleanup");
}

/// A single unreachable source is a timeout, mapped to `504 Gateway Timeout`
/// (clause 6.3.16). Confirms REQ-HTTP-26.
#[tokio::test]
async fn dist_http_single_source_timeout_returns_504() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let entity_id = topology.id("Vehicle:timeout");
    let reg_id = topology.id("registration:timeout");
    let mock = ContextSourceAdapter::start().await;
    let endpoint = mock.url("");
    mock.shutdown().await;

    let registration = retrieval_source(&reg_id, &endpoint, &entity_id, "exclusive");
    assert_status(
        &topology
            .front()
            .client()
            .create_registration(&registration)
            .await
            .expect("register unreachable source"),
        201,
    );

    let response = topology
        .front()
        .client()
        .get_entity(&entity_id, "", "application/ld+json", Some(context_link()))
        .await
        .expect("retrieve");
    assert_status(&response, 504);

    topology.finish().await.expect("topology cleanup");
}

/// A single source reporting the resource absent maps to `404 Not Found`
/// (clause 6.3.16). Confirms REQ-HTTP-27.
#[tokio::test]
async fn dist_http_single_source_resource_absent_returns_404() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let entity_id = topology.id("Vehicle:absent");
    let reg_id = topology.id("registration:absent");
    let mock = ContextSourceAdapter::start().await;

    let registration = retrieval_source(&reg_id, &mock.url(""), &entity_id, "exclusive");
    assert_status(
        &topology.front().client().create_registration(&registration).await.expect("register source"),
        201,
    );
    mock.set_stub(Method::GET, &forwarded_entity_path(&entity_id), 404).await;

    let response = topology
        .front()
        .client()
        .get_entity(&entity_id, "", "application/ld+json", Some(context_link()))
        .await
        .expect("retrieve");
    assert_status(&response, 404);

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A single source failing with a definite error other than absence maps to
/// `502 Bad Gateway` (clause 6.3.16). Confirms REQ-HTTP-28.
#[tokio::test]
async fn dist_http_single_source_other_failure_returns_502() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let entity_id = topology.id("Vehicle:badgateway");
    let reg_id = topology.id("registration:badgateway");
    let mock = ContextSourceAdapter::start().await;

    let registration = retrieval_source(&reg_id, &mock.url(""), &entity_id, "exclusive");
    assert_status(
        &topology.front().client().create_registration(&registration).await.expect("register source"),
        201,
    );
    mock.set_stub(Method::GET, &forwarded_entity_path(&entity_id), 500).await;

    let response = topology
        .front()
        .client()
        .get_entity(&entity_id, "", "application/ld+json", Some(context_link()))
        .await
        .expect("retrieve");
    assert_status(&response, 502);

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A create spanning several endpoints where one fails is a multi-status result,
/// mapped to `207` (clause 6.3.16). Confirms REQ-HTTP-29.
#[tokio::test]
async fn dist_http_multi_endpoint_source_error_returns_207() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let entity_id = topology.id("Vehicle:multi-207");
    let ok_reg = topology.id("registration:multi-207-ok");
    let fail_reg = topology.id("registration:multi-207-fail");
    let ok_source = ContextSourceAdapter::start().await;
    let fail_source = ContextSourceAdapter::start().await;

    let ok = provision_source(&ok_reg, &ok_source.url(""), &entity_id, "inclusive", &["createEntity"]);
    assert_status(&topology.front().client().create_registration(&ok).await.expect("register ok source"), 201);
    let fail = provision_source(&fail_reg, &fail_source.url(""), &entity_id, "inclusive", &["createEntity"]);
    assert_status(
        &topology.front().client().create_registration(&fail).await.expect("register failing source"),
        201,
    );
    ok_source.set_stub(Method::POST, FORWARDED_ENTITIES_PATH, 201).await;
    fail_source.set_stub(Method::POST, FORWARDED_ENTITIES_PATH, 500).await;

    let response = topology
        .front()
        .client()
        .post_entity(&vehicle(&entity_id, 7))
        .with_ld_json()
        .send()
        .await
        .expect("distributed create");
    assert_status(&response, 207);

    ok_source.shutdown().await;
    fail_source.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// An auxiliary registration is read-only, so an unsafe (write) method is never
/// forwarded to it and is handled locally (clause 5.7.3.4). Confirms REQ-HTTP-30.
#[tokio::test]
async fn dist_http_auxiliary_registration_rejects_unsafe_method() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let entity_id = topology.id("Vehicle:aux-unsafe");
    let reg_id = topology.id("registration:aux-unsafe");
    let mock = ContextSourceAdapter::start().await;

    let registration = provision_source(&reg_id, &mock.url(""), &entity_id, "auxiliary", &["createEntity", "updateAttrs", "appendAttrs"]);
    assert_status(
        &topology
            .front()
            .client()
            .create_registration(&registration)
            .await
            .expect("register auxiliary source"),
        201,
    );
    mock.set_stub(Method::POST, FORWARDED_ENTITIES_PATH, 500).await;

    let response = topology
        .front()
        .client()
        .post_entity(&vehicle(&entity_id, 11))
        .with_ld_json()
        .send()
        .await
        .expect("local create");
    assert_status(&response, 201);
    assert_eq!(
        mock.get_count(Method::POST, FORWARDED_ENTITIES_PATH).await,
        0,
        "an auxiliary source must not receive a forwarded unsafe method"
    );

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// An unsafe method forwarded to a failing source produces an informative
/// response body rather than a silent success (clause 6.3.16). Confirms REQ-HTTP-32.
#[tokio::test]
async fn dist_http_unsafe_method_source_error_is_informative() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let entity_id = topology.id("Vehicle:unsafe-error");
    let reg_id = topology.id("registration:unsafe-error");
    let mock = ContextSourceAdapter::start().await;

    let registration = provision_source(&reg_id, &mock.url(""), &entity_id, "exclusive", &["createEntity"]);
    assert_status(
        &topology.front().client().create_registration(&registration).await.expect("register source"),
        201,
    );
    mock.set_stub(Method::POST, FORWARDED_ENTITIES_PATH, 500).await;

    let response = topology
        .front()
        .client()
        .post_entity(&vehicle(&entity_id, 13))
        .with_ld_json()
        .send()
        .await
        .expect("forwarded create");
    assert!(
        response.status.as_u16() != 201,
        "a failed forwarded write must not report a bare success, got {}",
        response.status.as_u16()
    );
    let body = json_body(&response);
    assert!(!body.is_null(), "the failed forwarded write must carry an informative body");

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A failed endpoint is declined for the duration of its cooldown before being
/// retried. Cooldown decline is not implemented, so this is a tracked gap.
/// Confirms REQ-HTTP-31.
#[ignore = "GAP REQ-HTTP-31: cooldown decline not enforced"]
#[tokio::test]
async fn dist_http_failed_endpoint_skipped_until_cooldown() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let entity_id = topology.id("Vehicle:cooldown");
    let reg_id = topology.id("registration:cooldown");
    let mock = ContextSourceAdapter::start().await;

    let registration = retrieval_source(&reg_id, &mock.url(""), &entity_id, "inclusive");
    assert_status(
        &topology.front().client().create_registration(&registration).await.expect("register source"),
        201,
    );
    mock.set_stub(Method::GET, &forwarded_entity_path(&entity_id), 500).await;

    let first = topology
        .front()
        .client()
        .get_entity(&entity_id, "", "application/ld+json", Some(context_link()))
        .await
        .expect("first retrieve marks the endpoint failed");
    assert_status(&first, 200);
    let second = topology
        .front()
        .client()
        .get_entity(&entity_id, "", "application/ld+json", Some(context_link()))
        .await
        .expect("second retrieve");
    assert_status(&second, 200);
    assert_eq!(
        mock.get_count(Method::GET, &forwarded_entity_path(&entity_id)).await,
        1,
        "the failed endpoint must be skipped during cooldown"
    );

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}
