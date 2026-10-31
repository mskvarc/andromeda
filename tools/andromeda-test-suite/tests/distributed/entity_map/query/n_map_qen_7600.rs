//! Entity map true answers 201 with a map of the local and peer entities.

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
    http::{headers::Accept, operation::EntityMapRequest},
    model::Entity,
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, EntitySource, RegistrationInfo, RegistrationMode},
};

/// Pins: Query Entities on broker A with `entityMap` true, over one local Entity and one Entity broker B serves
/// through an inclusive registration, answers 201 with an `NGSILD-EntityMap` header naming an `EntityMap` that
/// lists the local Entity and the peer Entity under that registration, retrievable on broker A
/// Clause: 5.7.2.4, 5.14, 6.3.15
#[gtest]
#[tokio::test]
async fn n_map_qen_7600_entity_map_true_answers_201_with_a_map_of_the_local_and_peer_entities() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let broker_b = broker::client(BrokerRole::B)?;
    let vehicle_2_id = namespace.urn("Vehicle", 2)?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 10)
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let vehicle_2 = Entity::builder()
        .id(vehicle_2_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 20)
        .build()?;
    broker_b.entities().create(&vehicle_2).send().await?.done()?;
    let registration = CSourceRegistration::builder(broker::peer(PeerRole::B)?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern_ending("Vehicle", "2")?)])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::QueryEntity])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    let entity_map_request = EntityMapRequest::returned();
    let page = broker
        .with_heads()
        .with_accept(Accept::any())
        .entities()
        .query()
        .entity_type(&vehicle_type)
        .entity_map(&entity_map_request)
        .send()
        .await?
        .expecting(StatusCode::CREATED)?;
    expect_that!(
        page.entity_map().valid().map(|map| map.as_iri_ref().as_str()),
        some(contains_regex("^urn:ngsi-ld:EntityMap:.+"))
    );
    expect_that!(page.items(), len(eq(2)));
    let map = page.entity_map().valid().or_fail()?;
    let map_id = map.id().or_fail()?;
    let map_2 = broker.with_accept(Accept::any()).entity_maps().retrieve(&map_id).send().await?;
    expect_that!(&map_2.id, eq(&map_id));
    expect_that!(map_2.entity_map.get(&vehicle_id), some(anything()));
    expect_that!(
        map_2.entity_map.get(&vehicle_2_id),
        some(contains(eq(&EntitySource::Registration(registration_id.clone()))))
    );
    let map_3 = broker.with_accept(Accept::any()).entity_maps().retrieve(&map_id).send().await?;
    expect_that!(&map_3.id, eq(&map_id));
    Ok(())
}
