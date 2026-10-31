//! A locally partial entity stays a candidate beside the peer that completes it.

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
    model::{Entity, NgsiLdMediaType},
    query::{EntitySplitting, q},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, EntityMapId, EntitySource, RegistrationInfo, RegistrationMode},
};

/// Pins: Create `EntityMap` for Query Entities with `splitEntities` true and q speed>50 over an Entity broker-a
/// holds only as fuel and broker-b holds as speed 60 maps it to @none and to the registration of broker-b, the
/// filter being left to the merged result
/// Clause: 5.14.4.4, 5.5.9.3, 5.7.2.4
#[gtest]
#[tokio::test]
async fn n_map_qen_7013_a_locally_partial_entity_stays_a_candidate_beside_the_peer_that_completes_it() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let broker_b = broker::client(BrokerRole::B)?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("fuel", 30)
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let vehicle_2 = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 60)
        .build()?;
    broker_b.entities().create(&vehicle_2).send().await?.done()?;
    let registration = CSourceRegistration::builder(broker::peer(PeerRole::B)?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id(vehicle_id.clone())])
            .property_names(["speed"])
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
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entity_maps()
        .create()
        .entity_type(&vehicle_type)
        .ids(vec![vehicle_id.clone()])
        .q(q::attr("speed").gt(50))
        .split_entities(EntitySplitting::Split)
        .send()
        .await?
        .expecting(StatusCode::CREATED)?;
    let map = created.body();
    expect_that!(map.entity_map.get(&vehicle_id), some(len(eq(2))));
    expect_that!(map.entity_map.get(&vehicle_id), some(contains(eq(&EntitySource::Local))));
    expect_that!(
        map.entity_map.get(&vehicle_id),
        some(contains(eq(&EntitySource::Registration(registration_id.clone()))))
    );
    expect_that!(
        map.linked_maps.get(&registration_id).map(EntityMapId::as_str),
        some(starts_with("urn:ngsi-ld:EntityMap:"))
    );
    Ok(())
}
