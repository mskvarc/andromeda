//! A split capture decides q on the entity assembled from its parts.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    cleanup::Cleanup,
    config::BrokerRole,
    entity_matcher::has_attribute,
    identity_matcher::has_id,
    namespace::Namespace,
    outcome::Done,
    peer_role::PeerRole,
    snapshot_capture::{SnapshotQuery, await_query_result, await_status},
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::Entity,
    query::{EntitySplitting, q},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
    snapshot::{ExecutionStatus, QueryDocument, SettledStatus, Snapshot, SnapshotId, SnapshotStatus},
};

/// Pins: A Snapshot whose query by type has `splitEntities` true and q speed>50, over an inclusive registration of
/// peer broker B, captures the Entity whose local part holds only brand and whose speed of 70 B holds, merged, the
/// Entity B alone holds with speed 80 and the local one with speed 90, and none whose speed is 30, 20 or 10; a second
/// Snapshot with q speed>100 captures none of them and reports its query empty
/// Clause: 5.16.1.4, 5.7.2.4, 4.3.6, 5.2.42
#[gtest]
#[tokio::test]
async fn n_snp_cre_8184_a_split_capture_decides_q_on_the_entity_assembled_from_its_parts() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let broker_b = broker::client(BrokerRole::B)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_2_id = namespace.urn("Vehicle", 2)?;
    let vehicle_3_id = namespace.urn("Vehicle", 3)?;
    let vehicle_4_id = namespace.urn("Vehicle", 4)?;
    let vehicle_5_id = namespace.urn("Vehicle", 5)?;
    let vehicle_6_id = namespace.urn("Vehicle", 6)?;
    let registration_id = namespace.registration_id(1)?;
    let snapshot_id = format!("urn:ngsi-ld:Snapshot:{namespace}-1").parse::<SnapshotId>()?;
    let snapshot_2_id = format!("urn:ngsi-ld:Snapshot:{namespace}-2").parse::<SnapshotId>()?;
    for (id, brand) in [(&vehicle_id, "Mercedes"), (&vehicle_2_id, "Volvo")] {
        let part = Entity::builder()
            .id(id.clone())
            .entity_type(vehicle_type.clone())
            .property("brand", brand)
            .build()?;
        broker.entities().create(&part).send().await?.done()?;
    }
    for (id, speed) in [(&vehicle_5_id, 90), (&vehicle_6_id, 10)] {
        let local = Entity::builder()
            .id(id.clone())
            .entity_type(vehicle_type.clone())
            .property("speed", speed)
            .build()?;
        broker.entities().create(&local).send().await?.done()?;
    }
    for (id, speed) in [(&vehicle_id, 70), (&vehicle_2_id, 30), (&vehicle_3_id, 80), (&vehicle_4_id, 20)] {
        let held = Entity::builder()
            .id(id.clone())
            .entity_type(vehicle_type.clone())
            .property("speed", speed)
            .build()?;
        broker_b.entities().create(&held).send().await?.done()?;
    }
    let registration = CSourceRegistration::builder(broker::peer(PeerRole::B)?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::QueryEntity])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    let snapshot = Snapshot::builder()
        .id(snapshot_id.clone())
        .query(
            QueryDocument::builder()
                .entities([vehicle_type.as_str()])
                .q(q::attr("speed").gt(50).build()?)
                .split_entities(EntitySplitting::Split)
                .build()?,
        )
        .build()?;
    broker.snapshots().create(&snapshot).send().await?;
    let snapshot_2 = Snapshot::builder()
        .id(snapshot_2_id.clone())
        .query(
            QueryDocument::builder()
                .entities([vehicle_type.as_str()])
                .q(q::attr("speed").gt(100).build()?)
                .split_entities(EntitySplitting::Split)
                .build()?,
        )
        .build()?;
    broker.snapshots().create(&snapshot_2).send().await?;
    await_status(&broker, &snapshot_id, SnapshotStatus::Settled(SettledStatus::Success)).await?;
    let page = broker
        .with_accept(Accept::any())
        .on_snapshot(&snapshot_id)
        .entities()
        .query()
        .entity_type(&vehicle_type)
        .send()
        .await?;
    expect_that!(
        page.items(),
        unordered_elements_are![
            all!(
                has_id(&vehicle_id),
                has_attribute("speed", any_attribute().value(70)),
                has_attribute("brand", any_attribute().value("Mercedes"))
            ),
            all!(has_id(&vehicle_3_id), has_attribute("speed", any_attribute().value(80))),
            all!(has_id(&vehicle_5_id), has_attribute("speed", any_attribute().value(90)))
        ]
    );
    let detail = await_query_result(&broker, &snapshot_2_id, SettledStatus::Empty, SnapshotQuery::Entity(0)).await?;
    expect_that!(detail.result_status, eq(ExecutionStatus::Empty));
    let page_2 = broker
        .with_accept(Accept::any())
        .on_snapshot(&snapshot_2_id)
        .entities()
        .query()
        .entity_type(&vehicle_type)
        .send()
        .await?;
    expect_that!(page_2.items(), is_empty());
    broker.snapshots().delete(&snapshot_id).send().await?;
    broker.snapshots().delete(&snapshot_2_id).send().await?;
    Ok(())
}
