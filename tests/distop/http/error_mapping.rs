//! Single-source and multi-source error mapping (clause 6.3.17): when an HTTP unsafe method resolves to exactly one exclusive or redirect source holding all of the data, that source's outcome maps onto the client status (508 loop, 504 timeout, 404 absent, 502 other), while a multi-endpoint operation with a failing member yields `207`. Distributed GET failures never become status codes: clause 6.3.17 routes them to `NGSILD-Warning` 199 on a successful response. Auxiliary registrations never advertise unsafe methods (clause 5.9.2.4).

use crate::{
    context_source_adapter::ContextSourceAdapter,
    entity::vehicle,
    http::forwarding::{FORWARDED_ENTITIES_PATH, context_link, forwarded_entity_path, provision_source, retrieval_source},
    oracle::{assert_status, has_warning, json_body},
    registration::CsrBuilder,
    topology::{self, BrokerRole},
};
use pretty_assertions::assert_eq;
use reqwest::Method;
use serde_json::json;

/// An endpoint no packet reaches, so a forwarded request only ends by its deadline.
const BLACKHOLE_ENDPOINT: &str = "http://10.255.255.1:9";

/// A single exclusive source registered back onto the broker itself is a loop, mapped to `508 Loop Detected` for an unsafe method (clause 6.3.17).
#[tokio::test]
async fn dist_http_single_source_loop_back_returns_508() {
    let topology = topology::single().await.expect("single-broker topology");
    let front = topology.broker(BrokerRole::A).expect("topology broker A");
    let entity_id = topology.id("Vehicle:loop");
    let reg_id = topology.id("registration:loop");

    let loop_back = CsrBuilder::new(&reg_id, front.peer_url())
        .tenant(topology.tenant())
        .mode("exclusive")
        .entity_id(&entity_id)
        .property_names(&["speed"])
        .operations(&["createEntity"])
        .alias(front.alias())
        .build();
    assert_status(&front.client().create_registration(&loop_back).await.expect("register loop-back source"), 201);

    let response = front
        .client()
        .post_entity(&vehicle(&entity_id, 5))
        .with_ld_json()
        .send()
        .await
        .expect("looping create");
    assert_status(&response, 508);
    let body = json_body(&response);
    assert_eq!(
        body["type"].as_str(),
        Some("https://uri.etsi.org/ngsi-ld/errors/LoopDetected"),
        "a 508 carries the dedicated LoopDetected type, not InternalError: {body}"
    );
    assert_eq!(body["title"].as_str(), Some("Loop detected"), "the title matches the dedicated type: {body}");

    topology.finish().await.expect("topology cleanup");
}

/// A single redirect source registered back onto the broker itself is a loop, mapped to `508 Loop Detected` for an unsafe method exactly as the exclusive case is: clause 6.3.17 scopes 508 to "the single registered source and tenant is registered to redirect back on to the Context Broker", which covers both the exclusive and redirect single-source shapes (clause 6.3.18).
#[tokio::test]
async fn dist_http_single_source_redirect_loop_back_returns_508() {
    let topology = topology::single().await.expect("single-broker topology");
    let front = topology.broker(BrokerRole::A).expect("topology broker A");
    let entity_id = topology.id("Vehicle:redirect-loop");
    let reg_id = topology.id("registration:redirect-loop");

    let loop_back = CsrBuilder::new(&reg_id, front.peer_url())
        .tenant(topology.tenant())
        .mode("redirect")
        .entity_id(&entity_id)
        .property_names(&["speed"])
        .operations(&["createEntity"])
        .alias(front.alias())
        .build();
    assert_status(&front.client().create_registration(&loop_back).await.expect("register loop-back source"), 201);

    let response = front
        .client()
        .post_entity(&vehicle(&entity_id, 5))
        .with_ld_json()
        .send()
        .await
        .expect("looping create");
    assert_status(&response, 508);
    let body = json_body(&response);
    assert_eq!(
        body["type"].as_str(),
        Some("https://uri.etsi.org/ngsi-ld/errors/LoopDetected"),
        "a redirect loop carries the dedicated LoopDetected type, not InternalError: {body}"
    );
    assert_eq!(body["title"].as_str(), Some("Loop detected"), "the title matches the dedicated type: {body}");

    topology.finish().await.expect("topology cleanup");
}

/// A single source that does not answer within the registration's timeout is mapped to `504 Gateway Timeout` for an unsafe method (clause 6.3.17).
#[tokio::test]
async fn dist_http_single_source_timeout_returns_504() {
    let topology = topology::single().await.expect("single-broker topology");
    let entity_id = topology.id("Vehicle:timeout");
    let reg_id = topology.id("registration:timeout");

    let registration = CsrBuilder::new(&reg_id, BLACKHOLE_ENDPOINT)
        .mode("exclusive")
        .entity_id(&entity_id)
        .property_names(&["speed"])
        .operations(&["createEntity"])
        .management(json!({"timeout": 500}))
        .build();
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .create_registration(&registration)
            .await
            .expect("register stalling source"),
        201,
    );

    let response = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .post_entity(&vehicle(&entity_id, 5))
        .with_ld_json()
        .send()
        .await
        .expect("forwarded create");
    assert_status(&response, 504);
    let body = json_body(&response);
    assert_eq!(
        body["type"].as_str(),
        Some("https://uri.etsi.org/ngsi-ld/errors/GatewayTimeout"),
        "a 504 carries the dedicated GatewayTimeout type, not InternalError: {body}"
    );
    assert_eq!(body["title"].as_str(), Some("Gateway timeout"), "the title matches the dedicated type: {body}");

    topology.finish().await.expect("topology cleanup");
}

/// A single source reporting the resource absent maps to `404 Not Found` for an unsafe method (clause 6.3.17).
#[tokio::test]
async fn dist_http_single_source_resource_absent_returns_404() {
    let topology = topology::single().await.expect("single-broker topology");
    let entity_id = topology.id("Vehicle:absent");
    let reg_id = topology.id("registration:absent");
    let mock = ContextSourceAdapter::start().await;

    let registration = provision_source(&reg_id, &mock.url(""), &entity_id, "Vehicle", "exclusive", &["createEntity"]);
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .create_registration(&registration)
            .await
            .expect("register source"),
        201,
    );
    mock.set_stub(Method::POST, FORWARDED_ENTITIES_PATH, 404).await;

    let response = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .post_entity(&vehicle(&entity_id, 5))
        .with_ld_json()
        .send()
        .await
        .expect("forwarded create");
    assert_status(&response, 404);

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A single source failing with a definite error other than absence maps to `502 Bad Gateway` for an unsafe method (clause 6.3.17).
#[tokio::test]
async fn dist_http_single_source_other_failure_returns_502() {
    let topology = topology::single().await.expect("single-broker topology");
    let entity_id = topology.id("Vehicle:badgateway");
    let reg_id = topology.id("registration:badgateway");
    let mock = ContextSourceAdapter::start().await;

    let registration = provision_source(&reg_id, &mock.url(""), &entity_id, "Vehicle", "exclusive", &["createEntity"]);
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .create_registration(&registration)
            .await
            .expect("register source"),
        201,
    );
    mock.set_stub(Method::POST, FORWARDED_ENTITIES_PATH, 500).await;

    let response = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .post_entity(&vehicle(&entity_id, 5))
        .with_ld_json()
        .send()
        .await
        .expect("forwarded create");
    assert_status(&response, 502);
    let body = json_body(&response);
    assert_eq!(
        body["type"].as_str(),
        Some("https://uri.etsi.org/ngsi-ld/errors/BadGateway"),
        "a 502 carries the dedicated BadGateway type, not InternalError: {body}"
    );
    assert_eq!(body["title"].as_str(), Some("Bad gateway"), "the title matches the dedicated type: {body}");

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A distributed GET whose single exclusive source loops back onto the broker is not a 508: clause 6.3.17 reports the loop as `NGSILD-Warning` 199 on the response.
#[tokio::test]
async fn dist_http_get_loop_back_returns_199_warning_not_508() {
    let topology = topology::single().await.expect("single-broker topology");
    let front = topology.broker(BrokerRole::A).expect("topology broker A");
    let entity_id = topology.id("Vehicle:get-loop");
    let reg_id = topology.id("registration:get-loop");

    let loop_back = CsrBuilder::new(&reg_id, front.peer_url())
        .tenant(topology.tenant())
        .mode("exclusive")
        .entity_id(&entity_id)
        .property_names(&["speed"])
        .operations(&["retrieveEntity"])
        .alias(front.alias())
        .build();
    assert_status(&front.client().create_registration(&loop_back).await.expect("register loop-back source"), 201);

    let response = front
        .client()
        .get_entity(&entity_id, "", "application/ld+json", Some(context_link()))
        .await
        .expect("looping retrieve");
    assert!(
        response.status.as_u16() < 500,
        "a GET loop is a warning, not a 5xx: got {}",
        response.status.as_u16()
    );
    assert!(has_warning(&response, 199), "the loop is reported as NGSILD-Warning 199");

    topology.finish().await.expect("topology cleanup");
}

/// A distributed GET whose single exclusive source times out is not a 504: clause 6.3.17 reports the missing response as `NGSILD-Warning` 199.
#[tokio::test]
async fn dist_http_get_timeout_returns_199_warning_not_504() {
    let topology = topology::single().await.expect("single-broker topology");
    let entity_id = topology.id("Vehicle:get-timeout");
    let reg_id = topology.id("registration:get-timeout");

    let registration = CsrBuilder::new(&reg_id, BLACKHOLE_ENDPOINT)
        .mode("exclusive")
        .entity_id(&entity_id)
        .property_names(&["speed"])
        .operations(&["retrieveEntity"])
        .management(json!({"timeout": 500}))
        .build();
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .create_registration(&registration)
            .await
            .expect("register stalling source"),
        201,
    );

    let response = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .get_entity(&entity_id, "", "application/ld+json", Some(context_link()))
        .await
        .expect("retrieve");
    assert!(
        response.status.as_u16() < 500,
        "a GET timeout is a warning, not a 5xx: got {}",
        response.status.as_u16()
    );
    assert!(has_warning(&response, 199), "the timeout is reported as NGSILD-Warning 199");

    topology.finish().await.expect("topology cleanup");
}

/// A create spanning several endpoints where one fails is a multi-status result, mapped to `207` (clause 6.3.17).
#[tokio::test]
async fn dist_http_multi_endpoint_source_error_returns_207() {
    let topology = topology::single().await.expect("single-broker topology");
    let entity_id = topology.id("Vehicle:multi-207");
    let ok_reg = topology.id("registration:multi-207-ok");
    let fail_reg = topology.id("registration:multi-207-fail");
    let ok_source = ContextSourceAdapter::start().await;
    let fail_source = ContextSourceAdapter::start().await;

    let ok = provision_source(&ok_reg, &ok_source.url(""), &entity_id, "Vehicle", "inclusive", &["createEntity"]);
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .create_registration(&ok)
            .await
            .expect("register ok source"),
        201,
    );
    let fail = provision_source(&fail_reg, &fail_source.url(""), &entity_id, "Vehicle", "inclusive", &["createEntity"]);
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .create_registration(&fail)
            .await
            .expect("register failing source"),
        201,
    );
    ok_source.set_stub(Method::POST, FORWARDED_ENTITIES_PATH, 201).await;
    fail_source.set_stub(Method::POST, FORWARDED_ENTITIES_PATH, 500).await;

    let response = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
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

/// An auxiliary registration is read-only, so an unsafe (write) method is never forwarded to it and is handled locally (clause 4.3.6.2).
#[tokio::test]
async fn dist_http_auxiliary_registration_rejects_unsafe_method() {
    let topology = topology::single().await.expect("single-broker topology");
    let entity_id = topology.id("Vehicle:aux-unsafe");
    let reg_id = topology.id("registration:aux-unsafe");
    let mock = ContextSourceAdapter::start().await;

    let registration = provision_source(&reg_id, &mock.url(""), &entity_id, "Vehicle", "auxiliary", &["retrieveEntity", "queryEntity"]);
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .create_registration(&registration)
            .await
            .expect("register auxiliary source"),
        201,
    );
    mock.set_stub(Method::POST, FORWARDED_ENTITIES_PATH, 500).await;

    let response = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .post_entity(&vehicle(&entity_id, 11))
        .with_ld_json()
        .send()
        .await
        .expect("local create");
    assert_status(&response, 201);
    assert_eq!(
        mock.get_count(Method::POST, FORWARDED_ENTITIES_PATH),
        0,
        "an auxiliary source must not receive a forwarded unsafe method"
    );

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// An unsafe method forwarded to a failing source produces an informative response body rather than a silent success (clause 6.3.17).
#[tokio::test]
async fn dist_http_unsafe_method_source_error_is_informative() {
    let topology = topology::single().await.expect("single-broker topology");
    let entity_id = topology.id("Vehicle:unsafe-error");
    let reg_id = topology.id("registration:unsafe-error");
    let mock = ContextSourceAdapter::start().await;

    let registration = provision_source(&reg_id, &mock.url(""), &entity_id, "Vehicle", "exclusive", &["createEntity"]);
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .create_registration(&registration)
            .await
            .expect("register source"),
        201,
    );
    mock.set_stub_with_body(Method::POST, FORWARDED_ENTITIES_PATH, 403, &json!({"detail": "no access"}))
        .await;

    let response = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .post_entity(&vehicle(&entity_id, 13))
        .with_ld_json()
        .send()
        .await
        .expect("forwarded create");
    assert_status(&response, 502);
    let body = json_body(&response);
    let detail = body["detail"].as_str().unwrap_or_default();
    assert!(
        detail.contains("403") && detail.contains("no access"),
        "the failed forwarded write carries the source's detail: {body}"
    );

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A failed endpoint whose registration declares a `cooldown` is declined for that period, with `NGSILD-Warning` 199 standing in for its answer (clauses 5.2.34 and 6.3.17).
#[tokio::test]
async fn dist_http_failed_endpoint_skipped_until_cooldown() {
    let topology = topology::single().await.expect("single-broker topology");
    let entity_id = topology.id("Vehicle:cooldown");
    let reg_id = topology.id("registration:cooldown");
    let mock = ContextSourceAdapter::start().await;

    let mut registration = retrieval_source(&reg_id, &mock.url(""), &entity_id, "Vehicle", "inclusive");
    registration["management"] = json!({"cooldown": 60_000});
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .create_registration(&registration)
            .await
            .expect("register source"),
        201,
    );
    mock.set_stub(Method::GET, &forwarded_entity_path(&entity_id), 500).await;

    let first = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .get_entity(&entity_id, "", "application/ld+json", Some(context_link()))
        .await
        .expect("first retrieve marks the endpoint failed");
    assert!(has_warning(&first, 299), "the failing source is reported as NGSILD-Warning 299");
    let second = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .get_entity(&entity_id, "", "application/ld+json", Some(context_link()))
        .await
        .expect("second retrieve");
    assert!(has_warning(&second, 199), "the declined source is reported as NGSILD-Warning 199");
    assert_eq!(
        mock.get_count(Method::GET, &forwarded_entity_path(&entity_id)),
        1,
        "the failed endpoint must be skipped during cooldown"
    );

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}
