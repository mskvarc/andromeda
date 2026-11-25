//! A split capture decides a geoquery on the entity assembled from its parts.

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
    snapshot_capture::await_status,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{
        Entity,
        geometry::{Position, Shape},
    },
    query::{EntitySplitting, geoquery},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
    snapshot::{QueryDocument, SettledStatus, Snapshot, SnapshotId, SnapshotStatus},
};

/// Pins: A Snapshot whose query by type has `splitEntities` true and a near geoquery around Madrid, over an inclusive
/// registration of peer broker B, captures the Entity whose local part holds only brand and whose location in Madrid
/// B holds, merged, the Entity B alone holds in Madrid and the local one in Madrid, and none located in Berlin
/// Clause: 5.16.1.4, 5.7.2.4, 4.3.6, 4.10
#[gtest]
#[tokio::test]
async fn n_snp_cre_8185_a_split_capture_decides_a_geoquery_on_the_entity_assembled_from_its_parts() -> Result<()> {
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
    let madrid = Shape::Point(Position::try_new(-3.7038, 40.4168)?);
    let berlin = Shape::Point(Position::try_new(13.405, 52.52)?);
    for (id, brand) in [(&vehicle_id, "Mercedes"), (&vehicle_2_id, "Volvo")] {
        let part = Entity::builder()
            .id(id.clone())
            .entity_type(vehicle_type.clone())
            .property("brand", brand)
            .build()?;
        broker.entities().create(&part).send().await?.done()?;
    }
    for (id, place) in [(&vehicle_5_id, &madrid), (&vehicle_6_id, &berlin)] {
        let local = Entity::builder()
            .id(id.clone())
            .entity_type(vehicle_type.clone())
            .geo_property("location", place.clone())
            .build()?;
        broker.entities().create(&local).send().await?.done()?;
    }
    for (id, place) in [
        (&vehicle_id, &madrid),
        (&vehicle_2_id, &berlin),
        (&vehicle_3_id, &madrid),
        (&vehicle_4_id, &berlin),
    ] {
        let held = Entity::builder()
            .id(id.clone())
            .entity_type(vehicle_type.clone())
            .geo_property("location", place.clone())
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
                .geo_q(geoquery::near_max(3000, madrid).build()?)
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
        unordered_elements_are![
            all!(
                has_id(&vehicle_id),
                has_attribute("location", any_attribute()),
                has_attribute("brand", any_attribute().value("Mercedes"))
            ),
            all!(has_id(&vehicle_3_id), has_attribute("location", any_attribute())),
            all!(has_id(&vehicle_5_id), has_attribute("location", any_attribute()))
        ]
    );
    broker.snapshots().delete(&snapshot_id).send().await?;
    Ok(())
}
