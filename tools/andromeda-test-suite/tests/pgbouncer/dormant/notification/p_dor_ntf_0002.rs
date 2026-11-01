//! A failed durable delivery is retried after its tenant is evicted.

use andromeda_test_suite::{
    broker,
    config::BrokerRole,
    eviction::evict_by_pressure,
    namespace::Namespace,
    notification_sink::NotificationSink,
    problem::answered,
    retry_chain::RetryChain,
    tenant_set::TenantSet,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    model::{EntityFragment, NgsiLdMediaType},
    subscription::{Endpoint, NotificationParams, Subscription},
};
use wiremock::ResponseTemplate;

/// Pins: an Update Attributes answered 204, whose notification fails at every attempt with 500, is attempted again
/// after its tenant was evicted under the pressure of 40 other tenants and seen dormant
/// Clause: 5.8.6
/// Tenant: private (the subject is a whole tenant leaving the runtime cache)
/// Recorded: Andromeda configuration, not v1.9.1: the broker delivers durably, retrying a failed delivery ten times at
/// intervals doubling from 5 s, and loads an evicted tenant again for a retry that falls due
#[gtest]
#[tokio::test]
async fn p_dor_ntf_0002_a_failed_durable_delivery_is_retried_after_its_tenant_is_evicted() -> Result<()> {
    let namespace = Namespace::fresh();
    let dormant = broker::client(BrokerRole::Dormant)?;
    let sink = NotificationSink::start().await?;
    sink.answer("failing", ResponseTemplate::new(500)).await?;
    let set = TenantSet::provision(&dormant, &namespace, 1, 1).await?;
    let member = set.member(0);
    let subscription = Subscription::builder()
        .id(namespace.subscription_id("failing")?)
        .entities([namespace.entity_type("Device")?.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint("failing")?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    member.client.subscriptions().create(&subscription).send().await?;
    let change = EntityFragment::builder().property("reading", 7).build()?;
    let changed = member.client.with_responses().entities().update_attributes(set.device(), &change).send().await;
    expect_that!(&changed, answered(StatusCode::NO_CONTENT));
    let chain = RetryChain::started(&sink, "failing").await?;
    let dormant_at = evict_by_pressure(&dormant, &member.tenant).await?;
    let attempts = chain.attempts_after(dormant_at).await?;
    verify_that!(attempts.total, gt(attempts.before))
}
