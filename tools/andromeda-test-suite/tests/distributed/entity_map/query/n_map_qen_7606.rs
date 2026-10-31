//! Attrs keeps the local and peer holders of the attribute.

use andromeda_test_suite::{
    broker,
    cleanup::Cleanup,
    config::{BrokerRole, PeerRole},
    namespace::Namespace,
    outcome::Done,
    received_status::ExpectedStatus,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::Entity,
    query::EntitySplitting,
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Create `EntityMap` on broker A by type with attrs colour and `splitEntities` false, over two local
/// Entities and two broker B serves through an inclusive registration, answers 201 with a map of the local and the
/// peer Entity holding colour only
/// Clause: 5.14.4.4, 6.4.3.2
#[gtest]
#[tokio::test]
async fn n_map_qen_7606_attrs_keeps_the_local_and_peer_holders_of_the_attribute() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_2_id = namespace.urn("Vehicle", 2)?;
    let broker_b = broker::client(BrokerRole::B)?;
    let vehicle_3_id = namespace.urn("Vehicle", 3)?;
    let vehicle_4_id = namespace.urn("Vehicle", 4)?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle = Entity::builder()
        .id(vehicle_id)
        .entity_type(vehicle_type.clone())
        .property("speed", 100)
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let vehicle_2 = Entity::builder()
        .id(vehicle_2_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 20)
        .property("colour", "red")
        .build()?;
    broker.entities().create(&vehicle_2).send().await?.done()?;
    let vehicle_3 = Entity::builder()
        .id(vehicle_3_id)
        .entity_type(vehicle_type.clone())
        .property("speed", 100)
        .build()?;
    broker_b.entities().create(&vehicle_3).send().await?.done()?;
    let vehicle_4 = Entity::builder()
        .id(vehicle_4_id.clone())
        .entity_type(vehicle_type.clone())
        .property("colour", "red")
        .build()?;
    broker_b.entities().create(&vehicle_4).send().await?.done()?;
    let registration = CSourceRegistration::builder(broker::peer(PeerRole::B)?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern_ending("Vehicle", "[34]")?)])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::QueryEntity, DistributedOperation::CreateEntityMapQueryEntity])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    let created = broker
        .with_heads()
        .with_accept(Accept::any())
        .entity_maps()
        .create()
        .entity_type(&vehicle_type)
        .attrs(vec!["colour".parse()?])
        .split_entities(EntitySplitting::Whole)
        .send()
        .await?
        .expecting(StatusCode::CREATED)?;
    let map = created.body();
    expect_that!(&map.entity_map, len(eq(2)));
    expect_that!(map.entity_map.get(&vehicle_2_id), some(anything()));
    expect_that!(map.entity_map.get(&vehicle_4_id), some(anything()));
    Ok(())
}
