//! A failed delivery of a Snapshot branch is retried after its tenant is evicted.

use andromeda_test_suite::{
    broker,
    config::BrokerRole,
    eviction::evict_by_pressure,
    namespace::Namespace,
    notification_sink::NotificationSink,
    problem::answered,
    retry_chain::RetryChain,
    snapshot_capture::await_status,
    tenant_set::TenantSet,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    model::{Entity, NgsiLdMediaType},
    snapshot::{QueryDocument, SettledStatus, Snapshot, SnapshotId, SnapshotStatus},
    subscription::{Endpoint, NotificationParams, Subscription},
};
use wiremock::ResponseTemplate;

/// Pins: a Snapshot whose query selects nothing settles `empty` and publishes its branch; a Subscription and an
/// Entity created under that Snapshot (201), whose notification fails at every attempt with 500, are attempted again
/// after the tenant was evicted under the pressure of 40 other tenants and seen dormant, though nothing outside the
/// branch is due
/// Clause: 5.8.6, 5.16.1.4, 6.3.22
/// Tenant: private (the subject is a whole tenant leaving the runtime cache)
/// Recorded: Andromeda configuration, not v1.9.1: the broker delivers durably, retrying a failed delivery ten times at
/// intervals doubling from 5 s, and loads an evicted tenant again for a retry that falls due
#[gtest]
#[tokio::test]
async fn p_dor_ntf_0004_a_failed_delivery_of_a_snapshot_branch_is_retried_after_its_tenant_is_evicted() -> Result<()> {
    let namespace = Namespace::fresh();
    let dormant = broker::client(BrokerRole::Dormant)?;
    let sink = NotificationSink::start().await?;
    sink.answer("failing", ResponseTemplate::new(500)).await?;
    let set = TenantSet::provision(&dormant, &namespace, 1, 1).await?;
    let member = set.member(0);
    let snapshot_id = format!("urn:ngsi-ld:Snapshot:{namespace}-branch").parse::<SnapshotId>()?;
    let snapshot = Snapshot::builder()
        .id(snapshot_id.clone()) // clone: the branch is addressed by the Snapshot's id after it is built
        .query(QueryDocument::builder().entities([namespace.entity_type("Absent")?.as_str()]).build()?)
        .build()?;
    member.client.snapshots().create(&snapshot).send().await?;
    await_status(&member.client, &snapshot_id, SnapshotStatus::Settled(SettledStatus::Empty)).await?;
    let branch = member.client.with_responses();
    let branch = branch.on_snapshot(&snapshot_id);
    let branch_type = namespace.entity_type("BranchDevice")?;
    let subscription = Subscription::builder()
        .id(namespace.subscription_id("failing")?)
        .entities([branch_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint("failing")?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    let subscribed = branch.subscriptions().create(&subscription).send().await;
    expect_that!(&subscribed, answered(StatusCode::CREATED));
    let device = Entity::builder()
        .id(namespace.urn("BranchDevice", 1)?)
        .entity_type(branch_type)
        .property("reading", 1)
        .build()?;
    let created = branch.entities().create(&device).send().await;
    expect_that!(&created, answered(StatusCode::CREATED));
    let chain = RetryChain::started(&sink, "failing").await?;
    let dormant_at = evict_by_pressure(&dormant, &member.tenant).await?;
    let attempts = chain.attempts_after(dormant_at).await?;
    verify_that!(attempts.total, gt(attempts.before))
}
