//! Shared setup for CSR-subscription forwarding cases (clause 5.8.5): a subscription-supporting Context Source Registration pointing at an in-process mock, plus the stubs and accessors the forwarding assertions rest on. When a local subscription matches such a registration the broker provisions a reduced-copy subscription on the mock, which captures the forwarded request.

use crate::{context_source_adapter::ContextSourceAdapter, oracle::assert_status, registration::CsrBuilder, topology::BrokerRole};
use andromeda_test_harness::multi_broker::MultiBrokerTopology;
use andromeda_test_mocks::callback_server::CapturedRequest;
use axum::http::Method;
use serde_json::Value;
use std::time::Duration;

/// How long to wait for a forwarded subscription request before treating its
/// absence as a failed forward.
pub const FORWARD_TIMEOUT: Duration = Duration::from_secs(10);

/// Register an inclusive `Vehicle` registration on the front broker pointing at the mock, declaring `federationOps` so a matching local subscription drives a CSR-subscription forward to it.
pub async fn register_subscription_source(topology: &MultiBrokerTopology<BrokerRole>, endpoint: &str, entity_type: &str, registration_id: &str) {
    let registration = CsrBuilder::new(registration_id, endpoint)
        .mode("inclusive")
        .entity_type(entity_type)
        .operations(&["federationOps"])
        .build();
    let created = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .create_registration(&registration)
        .await
        .expect("register subscription source");
    assert_status(&created, 201);
}

/// Bring up a mock Context Source, stub its subscription endpoints, and register an inclusive `Vehicle` source on the front broker pointing at it.
pub async fn subscription_source(topology: &MultiBrokerTopology<BrokerRole>, entity_type: &str, registration_local: &str) -> ContextSourceAdapter {
    let mock = ContextSourceAdapter::start().await;
    stub_subscription_endpoints(&mock).await;
    register_subscription_source(topology, &mock.url(""), entity_type, &topology.id(registration_local)).await;
    mock
}

/// The forwarded subscriptions resource: the registration `endpoint` is the source origin and the broker appends the full NGSI-LD path to it (clause 6.3.15 / 4.3.6), so the mock must answer under `/ngsi-ld/v1`.
pub const FORWARDED_SUBSCRIPTIONS_PATH: &str = "/ngsi-ld/v1/subscriptions";

/// Stub the mock's subscription-management endpoints so a forwarded create, update, or delete receives a plausible success.
pub async fn stub_subscription_endpoints(mock: &ContextSourceAdapter) {
    mock.set_stub(Method::POST, FORWARDED_SUBSCRIPTIONS_PATH, 201).await;
    mock.set_stub(Method::PATCH, FORWARDED_SUBSCRIPTIONS_PATH, 204).await;
    mock.set_stub(Method::DELETE, FORWARDED_SUBSCRIPTIONS_PATH, 204).await;
}

/// The JSON subscription document carried by a forwarded request.
pub fn forwarded_subscription(request: &CapturedRequest) -> Value {
    request.json().expect("forwarded subscription body is JSON")
}
