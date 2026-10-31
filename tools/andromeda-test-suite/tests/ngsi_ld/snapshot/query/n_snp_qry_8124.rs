//! Snapshot scoped subscriptions over two tenants each notify once with their tenant and snapshot.

use andromeda_test_suite::{
    answered_status::AnsweredStatus,
    broker::{self, BrokerClient},
    header_condition::HeaderCondition,
    identity_matcher::has_id,
    namespace::Namespace,
    notification_sink::NotificationSink,
    snapshot_capture::await_status,
};
use futures::future::join_all;
use googletest::prelude::*;
use http::{HeaderName, StatusCode};
use ngsi_ld::{
    model::{Entity, EntityId, Input, Lossless, NgsiLdMediaType, Representation},
    snapshot::{QueryDocument, SettledStatus, Snapshot, SnapshotId, SnapshotStatus},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// The Snapshots created in each Tenant.
const SNAPSHOTS_PER_TENANT: usize = 4;

/// The Subscriptions created under each Snapshot.
const SUBSCRIPTIONS_PER_SNAPSHOT: usize = 8;

/// Pins: 64 Create Subscription requests sent at once with NGSILD-Snapshot, 8 under each of 4 Snapshots in each of
/// two Tenants, each to an endpoint of its own, each answer 201; 8 Create Entity requests sent at once, one under each
/// Snapshot, each answer 201, and each Subscription is notified exactly once, of the Entity created under its
/// Snapshot, with the NGSILD-Tenant and NGSILD-Snapshot headers naming its Tenant and Snapshot; 64 Delete
/// Subscription requests sent at once under each one's Tenant and Snapshot each answer 204
/// Clause: 6.3.22, 5.5.15, 5.8.6, 4.14
/// Tenant: private (the subject is Snapshot-scoped Subscriptions spread over two Tenants, each notification naming its
/// own)
#[gtest]
#[tokio::test]
async fn n_snp_qry_8124_snapshot_scoped_subscriptions_over_two_tenants_each_notify_once_with_their_tenant_and_snapshot() -> Result<()> {
    let namespace = Namespace::fresh();
    let front = broker::front()?;
    let sink = NotificationSink::start().await?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let tenant_namespaces = [Namespace::fresh(), Namespace::fresh()];
    let mut tenants = Vec::with_capacity(tenant_namespaces.len());
    for tenant_namespace in &tenant_namespaces {
        tenants.push(broker::private_tenant(&front, tenant_namespace).await?);
    }
    let tenants = tenants;
    let mut snapshots = Vec::with_capacity(tenants.len() * SNAPSHOTS_PER_TENANT);
    for (tenant_index, tenant) in tenants.iter().enumerate() {
        for snapshot_index in 0..SNAPSHOTS_PER_TENANT {
            let snapshot_id = format!("urn:ngsi-ld:Snapshot:{namespace}-{tenant_index}-{snapshot_index}").parse::<SnapshotId>()?;
            let snapshot = Snapshot::builder()
                // Clone: the builder owns the id the test keeps addressing.
                .id(snapshot_id.clone())
                .query(QueryDocument::builder().entities([vehicle_type.as_str()]).build()?)
                .build()?;
            tenant.snapshots().create(&snapshot).send().await?;
            await_status(tenant, &snapshot_id, SnapshotStatus::Settled(SettledStatus::Empty)).await?;
            snapshots.push((tenant_index, snapshot_id));
        }
    }
    let snapshots = snapshots;
    let responding = tenants.iter().map(BrokerClient::with_responses).collect::<Vec<_>>();
    let mut subscriptions = Vec::with_capacity(snapshots.len() * SUBSCRIPTIONS_PER_SNAPSHOT);
    for snapshot_position in 0..snapshots.len() {
        for subscription_index in 0..SUBSCRIPTIONS_PER_SNAPSHOT {
            let label = format!("{snapshot_position}-{subscription_index}");
            let subscription_id = namespace.subscription_id(&label)?;
            let subscription = Subscription::builder()
                // Clone: the builder owns the id the test matches notifications and deletes by.
                .id(subscription_id.clone())
                .entities([vehicle_type.as_str()])
                .notification(
                    NotificationParams::builder(Endpoint::builder(sink.endpoint(&label)?).accept(NgsiLdMediaType::Json).build()?)
                        .format(Representation::Normalized)
                        .build()?,
                )
                .build()?;
            subscriptions.push((snapshot_position, subscription_id, label, subscription));
        }
    }
    let subscriptions = subscriptions;
    let subscribed = join_all(subscriptions.iter().map(|(snapshot_position, _, _, subscription)| async {
        let (tenant_index, snapshot_id) = &snapshots[*snapshot_position];
        responding[*tenant_index]
            .on_snapshot(snapshot_id)
            .subscriptions()
            .create(subscription)
            .send()
            .await
    }))
    .await;
    expect_that!(
        subscribed.iter().map(AnsweredStatus::answered_status).collect::<Vec<_>>(),
        each(some(eq(&StatusCode::CREATED)))
    );
    let vehicle_ids = (0..snapshots.len())
        .map(|position| -> Result<EntityId> { Ok(namespace.urn("Vehicle", position)?) })
        .collect::<Result<Vec<_>>>()?;
    let vehicles = vehicle_ids
        .iter()
        .map(|vehicle_id| -> Result<Entity<Input>> {
            Ok(Entity::builder()
                // Clone: the builder owns the id the test matches notifications by.
                .id(vehicle_id.clone())
                // Clone: every vehicle owns its type.
                .entity_type(vehicle_type.clone())
                .property("speed", 1)
                .build()?)
        })
        .collect::<Result<Vec<_>>>()?;
    let created =
        join_all(snapshots.iter().zip(&vehicles).map(|((tenant_index, snapshot_id), vehicle)| async {
            responding[*tenant_index].on_snapshot(snapshot_id).entities().create(vehicle).send().await
        }))
        .await;
    expect_that!(
        created.iter().map(AnsweredStatus::answered_status).collect::<Vec<_>>(),
        each(some(eq(&StatusCode::CREATED)))
    );
    for (snapshot_position, subscription_id, label, _) in &subscriptions {
        let (tenant_index, snapshot_id) = &snapshots[*snapshot_position];
        let fields = [
            HeaderCondition::Is(HeaderName::from_static("ngsild-tenant"), tenant_namespaces[*tenant_index].token().to_owned()),
            HeaderCondition::Is(HeaderName::from_static("ngsild-snapshot"), snapshot_id.to_string()),
        ];
        sink.wait_for_at_least_matching(label.as_str(), &[], &fields, 1).await?;
        let received = sink.received(label.as_str()).await?;
        expect_that!(received, len(eq(1)));
        let notification = received.first().or_fail()?.entities(Lossless::Normalized)?;
        expect_that!(notification.subscription_id(), eq(subscription_id));
        expect_that!(notification.data().as_slice(), elements_are![has_id(&vehicle_ids[*snapshot_position])]);
    }
    let deleted = join_all(subscriptions.iter().map(|(snapshot_position, subscription_id, _, _)| async {
        let (tenant_index, snapshot_id) = &snapshots[*snapshot_position];
        responding[*tenant_index]
            .on_snapshot(snapshot_id)
            .subscriptions()
            .delete(subscription_id)
            .send()
            .await
    }))
    .await;
    expect_that!(
        deleted.iter().map(AnsweredStatus::answered_status).collect::<Vec<_>>(),
        each(some(eq(&StatusCode::NO_CONTENT)))
    );
    for (tenant_index, snapshot_id) in &snapshots {
        tenants[*tenant_index].snapshots().delete(snapshot_id).send().await?;
    }
    Ok(())
}
