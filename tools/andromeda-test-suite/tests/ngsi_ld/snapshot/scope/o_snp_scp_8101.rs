//! A context source registration subscription operation naming a snapshot is refused and changes nothing.

use andromeda_test_suite::{broker, config::SuiteConfig, namespace::Namespace, problem::answered, snapshot_refusal::refused_on_snapshot};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{
        headers::Accept,
        operation::{CreateSubscription, DeleteSubscription},
    },
    model::NgsiLdMediaType,
    snapshot::{QueryDocument, Snapshot, SnapshotId},
    subscription::{Endpoint, NotificationParams, RegistrationSubscription, Subscription},
};

/// Pins: Create and Delete Context Source Registration Subscription with NGSILD-Snapshot naming an existing Snapshot
/// answer 422 `OperationNotSupported` with the same header and a detail naming the core, temporal and
/// entity-subscription operations; the subscription the create names does not exist and the one the delete names
/// remains
/// Clause: 5.5.15, 6.3.22, 5.11.2, 5.11.6
/// Recorded: clause 5.5.15 permits on a Snapshot the operations of the Core and Temporal APIs alone and names no error
/// for another; Andromeda answers `OperationNotSupported`
#[gtest]
#[tokio::test]
async fn o_snp_scp_8101_a_registration_subscription_operation_naming_a_snapshot_is_refused_and_changes_nothing() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let notify = SuiteConfig::get()?.unreachable_origin().join("notify")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let snapshot_id = format!("urn:ngsi-ld:Snapshot:{namespace}-1").parse::<SnapshotId>()?;
    let kept_id = namespace.subscription_id("kept")?;
    let refused_id = namespace.subscription_id("refused")?;
    let snapshot = Snapshot::builder()
        .id(snapshot_id.clone())
        .query(QueryDocument::builder().entities([vehicle_type.as_str()]).build()?)
        .build()?;
    broker.snapshots().create(&snapshot).send().await?;
    let kept = Subscription::registration_builder()
        .id(kept_id.clone())
        .entities([vehicle_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(notify.clone()).build()?).build()?)
        .build()?;
    let refused = Subscription::registration_builder()
        .id(refused_id.clone())
        .entities([vehicle_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(notify).build()?).build()?)
        .build()?;
    broker.registration_subscriptions().create(&kept).send().await?;
    let on_snapshot = broker.on_snapshot(&snapshot_id);
    let created = broker
        .send_unchecked(on_snapshot.unchecked(&CreateSubscription { subscription: &refused })?)
        .await;
    expect_that!(&created, refused_on_snapshot(&snapshot_id));
    let deleted = broker
        .send_unchecked(on_snapshot.unchecked(&DeleteSubscription {
            id: &kept_id,
            kind: RegistrationSubscription,
        })?)
        .await;
    expect_that!(&deleted, refused_on_snapshot(&snapshot_id));
    let absent = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registration_subscriptions()
        .retrieve(&refused_id)
        .send()
        .await;
    expect_that!(&absent, answered(StatusCode::NOT_FOUND));
    let remaining = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registration_subscriptions()
        .retrieve(&kept_id)
        .send()
        .await?;
    expect_that!(remaining.id(), eq(&kept_id));
    broker.registration_subscriptions().delete(&kept_id).send().await?;
    broker.snapshots().delete(&snapshot_id).send().await?;
    Ok(())
}
