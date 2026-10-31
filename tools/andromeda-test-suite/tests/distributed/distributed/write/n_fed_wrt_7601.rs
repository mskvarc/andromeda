//! An exclusive attribute is created on the peer only and served from it.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    cleanup::Cleanup,
    config::{BrokerRole, PeerRole},
    entity_matcher::{has_attribute, lacks_attribute},
    namespace::Namespace,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::Entity,
    query::Locality,
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Create Entity on broker A of speed and `brandName`, under an exclusive registration of speed at broker B
/// advertising `createEntity` and `retrieveEntity`, answers 201; broker B holds speed and not `brandName`, broker A
/// holds `brandName` and not speed locally, and Retrieve Entity on broker A answers both
/// Clause: 5.6.1.4, 5.7.1.4, 4.3.6.3
#[gtest]
#[tokio::test]
async fn n_fed_wrt_7601_an_exclusive_attribute_is_created_on_the_peer_only_and_served_from_it() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let broker_b = broker::client(BrokerRole::B)?;
    let registration = CSourceRegistration::builder(broker::peer(PeerRole::B)?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id(vehicle_id.clone())])
            .property_names(["speed"])
            .build()?])
        .mode(RegistrationMode::Exclusive)
        .operations([DistributedOperation::CreateEntity, DistributedOperation::RetrieveEntity])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 88)
        .property("brandName", "Mercedes")
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let retrieved = broker_b.with_accept(Accept::any()).entities().retrieve(&vehicle_id).send().await?;
    let entity = retrieved.body();
    expect_that!(entity, lacks_attribute("brandName"));
    expect_that!(entity, has_attribute("speed", any_attribute().value(88)));
    let retrieved_2 = broker.with_accept(Accept::any()).entities().retrieve(&vehicle_id).send().await?;
    let entity_2 = retrieved_2.body();
    expect_that!(entity_2, has_attribute("speed", any_attribute().value(88)));
    expect_that!(entity_2, has_attribute("brandName", any_attribute().value("Mercedes")));
    let retrieved_3 = broker
        .with_accept(Accept::any())
        .entities()
        .retrieve(&vehicle_id)
        .locality(Locality::Local)
        .send()
        .await?;
    let entity_3 = retrieved_3.body();
    expect_that!(entity_3, lacks_attribute("speed"));
    expect_that!(entity_3, has_attribute("brandName", any_attribute().value("Mercedes")));
    Ok(())
}
