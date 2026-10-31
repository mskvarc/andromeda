//! A capture merges one entity held locally and by a peer.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    config::{BrokerRole, PeerRole},
    entity_matcher::{has_instance, has_instances},
    namespace::Namespace,
    outcome::Done,
    snapshot_capture::await_status,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType, Property},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
    snapshot::{QueryDocument, Snapshot, SnapshotId, SnapshotStatus},
};

/// Pins: A Snapshot on broker A over an Entity held locally with one `datasetId` and by peer broker B with another
/// ends success, and Retrieve Entity under it answers the Entity with both instances
/// Clause: 5.16.1.4, 5.7.2.4, 4.5.5, 4.3.6.2
#[gtest]
#[tokio::test]
async fn n_snp_cre_8151_a_capture_merges_one_entity_held_locally_and_by_a_peer() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let broker_b = broker::client(BrokerRole::B)?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration_id = namespace.registration_id(1)?;
    let local_dataset = namespace.dataset_id("Dataset", "local")?;
    let remote_dataset = namespace.dataset_id("Dataset", "remote")?;
    let snapshot_id = format!("urn:ngsi-ld:Snapshot:{namespace}-1").parse::<SnapshotId>()?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(10).dataset_id(local_dataset.clone()))
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let vehicle_remote = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(20).dataset_id(remote_dataset.clone()))
        .build()?;
    broker_b.entities().create(&vehicle_remote).send().await?.done()?;
    let registration = CSourceRegistration::builder(broker::peer(PeerRole::B)?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::QueryEntity])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    let snapshot = Snapshot::builder()
        .id(snapshot_id.clone())
        .query(QueryDocument::builder().entities([vehicle_type.as_str()]).build()?)
        .build()?;
    broker.snapshots().create(&snapshot).send().await?;
    await_status(&broker, &snapshot_id, "success".parse::<SnapshotStatus>()?).await?;
    let captured = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .on_snapshot(&snapshot_id)
        .entities()
        .retrieve(&vehicle_id)
        .send()
        .await?;
    let entity = captured.body();
    expect_that!(entity, has_instances("speed", 2));
    expect_that!(entity, has_instance("speed", any_attribute().dataset_id(local_dataset)));
    expect_that!(entity, has_instance("speed", any_attribute().dataset_id(remote_dataset)));
    broker.snapshots().delete(&snapshot_id).send().await?;
    broker.registrations().delete(&registration_id).send().await?;
    Ok(())
}
