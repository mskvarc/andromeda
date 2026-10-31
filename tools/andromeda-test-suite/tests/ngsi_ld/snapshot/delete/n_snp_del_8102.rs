//! Deleting a snapshot deletes its subscriptions.

use andromeda_test_suite::{broker, config::SuiteConfig, eventually::retried, namespace::Namespace, outcome::Done, problem::answered};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType, Representation},
    snapshot::{QueryDocument, Snapshot, SnapshotId, SnapshotStatus},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: Delete Snapshot of a Snapshot holding a Subscription with a pending notification answers 204; a Snapshot
/// created again under the same id holds no Subscription
/// Clause: 5.16.5.4, 5.5.15
#[gtest]
#[tokio::test]
async fn n_snp_del_8102_deleting_a_snapshot_deletes_its_subscriptions() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let unreachable = config.unreachable_origin().origin().ascii_serialization();
    let snapshot_id = format!("urn:ngsi-ld:Snapshot:{namespace}-1").parse::<SnapshotId>()?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let subscription_id = namespace.subscription_id(1)?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let snapshot = Snapshot::builder()
        .id(format!("urn:ngsi-ld:Snapshot:{namespace}-1").parse::<SnapshotId>()?)
        .query(QueryDocument::builder().entities([vehicle_type.as_str()]).build()?)
        .build()?;
    broker.snapshots().create(&snapshot).send().await?;
    retried(async || {
        let snapshot_2 = broker
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .snapshots()
            .retrieve(&snapshot_id)
            .send()
            .await?;
        verify_that!(snapshot_2.status(), eq("empty".parse::<SnapshotStatus>()?))?;
        Ok(())
    })
    .await?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([vehicle_type.as_str()])
        .notification(
            NotificationParams::builder(Endpoint::builder(format!("{unreachable}/notify")).accept(NgsiLdMediaType::Json).build()?)
                .format(Representation::Normalized)
                .build()?,
        )
        .build()?;
    broker.on_snapshot(&snapshot_id).subscriptions().create(&subscription).send().await?;
    let vehicle = Entity::builder()
        .id(vehicle_id)
        .entity_type(vehicle_type.clone())
        .property("speed", 1)
        .build()?;
    broker.on_snapshot(&snapshot_id).entities().create(&vehicle).send().await?.done()?;
    broker.snapshots().delete(&snapshot_id).send().await?;
    let answer = broker
        .with_responses()
        .with_accept(Accept::any())
        .on_snapshot(&snapshot_id)
        .subscriptions()
        .retrieve(&subscription_id)
        .send()
        .await;
    expect_that!(&answer, answered(StatusCode::NOT_FOUND));
    let snapshot_3 = Snapshot::builder()
        .id(format!("urn:ngsi-ld:Snapshot:{namespace}-1").parse::<SnapshotId>()?)
        .query(QueryDocument::builder().entities([vehicle_type.as_str()]).build()?)
        .build()?;
    broker.snapshots().create(&snapshot_3).send().await?;
    retried(async || {
        let snapshot_4 = broker
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .snapshots()
            .retrieve(&snapshot_id)
            .send()
            .await?;
        verify_that!(snapshot_4.status(), eq("empty".parse::<SnapshotStatus>()?))?;
        Ok(())
    })
    .await?;
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::any())
        .on_snapshot(&snapshot_id)
        .subscriptions()
        .retrieve(&subscription_id)
        .send()
        .await;
    expect_that!(&answer_2, answered(StatusCode::NOT_FOUND));
    let page = broker
        .with_accept(Accept::any())
        .on_snapshot(&snapshot_id)
        .subscriptions()
        .query()
        .send()
        .await?;
    expect_that!(page.items(), len(eq(0)));
    broker.snapshots().delete(&snapshot_id).send().await?;
    Ok(())
}
