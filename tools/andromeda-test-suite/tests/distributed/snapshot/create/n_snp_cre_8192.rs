//! A split capture decides q on the merged entity before attrs reduces it.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    cleanup::Cleanup,
    config::BrokerRole,
    entity_matcher::{has_attribute, lacks_attribute},
    identity_matcher::has_id,
    namespace::Namespace,
    outcome::Done,
    peer_role::PeerRole,
    snapshot_capture::await_status,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::Entity,
    query::{EntitySplitting, q},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
    snapshot::{QueryDocument, SettledStatus, Snapshot, SnapshotId, SnapshotStatus},
};

/// Pins: A Snapshot whose query by type has `splitEntities` true, attrs speed and q brand=="Mercedes", over an inclusive
/// registration of peer broker B, captures the Entity whose local part holds only brand Mercedes and whose speed 70
/// and fuelLevel B holds, reduced to speed, and neither the Entity B alone holds with brand Mercedes and no speed nor
/// the local one of brand Volvo
/// Clause: 5.16.1.4, 5.7.2.4, 5.7.2.5, 4.3.6
#[gtest]
#[tokio::test]
async fn n_snp_cre_8192_a_split_capture_decides_q_on_the_merged_entity_before_attrs_reduces_it() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let broker_b = broker::client(BrokerRole::B)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_2_id = namespace.urn("Vehicle", 2)?;
    let vehicle_3_id = namespace.urn("Vehicle", 3)?;
    let registration_id = namespace.registration_id(1)?;
    let snapshot_id = format!("urn:ngsi-ld:Snapshot:{namespace}-1").parse::<SnapshotId>()?;
    let part = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("brand", "Mercedes")
        .build()?;
    broker.entities().create(&part).send().await?.done()?;
    let local = Entity::builder()
        .id(vehicle_2_id.clone())
        .entity_type(vehicle_type.clone())
        .property("brand", "Volvo")
        .property("speed", 90)
        .build()?;
    broker.entities().create(&local).send().await?.done()?;
    let held = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 70)
        .property("fuelLevel", 40)
        .build()?;
    broker_b.entities().create(&held).send().await?.done()?;
    let unlisted = Entity::builder()
        .id(vehicle_3_id.clone())
        .entity_type(vehicle_type.clone())
        .property("brand", "Mercedes")
        .property("fuelLevel", 40)
        .build()?;
    broker_b.entities().create(&unlisted).send().await?.done()?;
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
                .attrs(["speed"])
                .q(q::attr("brand").eq("Mercedes").build()?)
                .split_entities(EntitySplitting::Split)
                .build()?,
        )
        .build()?;
    broker.snapshots().create(&snapshot).send().await?;
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
        unordered_elements_are![all!(
            has_id(&vehicle_id),
            has_attribute("speed", any_attribute().value(70)),
            lacks_attribute("brand"),
            lacks_attribute("fuelLevel")
        )]
    );
    broker.snapshots().delete(&snapshot_id).send().await?;
    Ok(())
}
