//! A posted query creates a map of local and peer entities.

use andromeda_test_suite::{
    broker,
    config::{BrokerRole, PeerRole},
    namespace::Namespace,
    outcome::Done,
    received_status::ExpectedStatus,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{headers::Accept, operation::QueryMethod},
    model::{Entity, NgsiLdMediaType},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: POST `entityMaps` on broker A with a Query by type, over a local Entity and one broker B serves through an
/// inclusive registration, answers 201 with an `NGSILD-EntityMap` header naming an `EntityMap` of both
/// Clause: 5.14.4.4, 6.31
#[gtest]
#[tokio::test]
async fn n_map_qen_7609_a_posted_query_creates_a_map_of_local_and_peer_entities() -> Result<()> {
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
        .property("speed", 1)
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let vehicle_2 = Entity::builder()
        .id(vehicle_2_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 2)
        .build()?;
    broker_b.entities().create(&vehicle_2).send().await?.done()?;
    let registration = CSourceRegistration::builder(broker::peer(PeerRole::B)?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern_ending("Vehicle", "2")?)])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::QueryEntity, DistributedOperation::CreateEntityMapQueryEntity])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    let created = broker
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entity_maps()
        .create()
        .entity_type(&vehicle_type)
        .method(QueryMethod::Post)
        .send()
        .await?
        .expecting(StatusCode::CREATED)?;
    let map = created.body();
    expect_that!(
        created.entity_map().valid().map(|header| header.as_iri_ref().as_str()),
        some(eq(map.id.as_str()))
    );
    expect_that!(&map.entity_map, len(eq(2)));
    expect_that!(map.entity_map.get(&vehicle_id), some(anything()));
    expect_that!(map.entity_map.get(&vehicle_2_id), some(anything()));
    Ok(())
}
