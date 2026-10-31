//! Attrs and key values apply to an entity served by the peer.

use andromeda_test_suite::{
    broker,
    cleanup::Cleanup,
    config::{BrokerRole, PeerRole},
    namespace::Namespace,
    outcome::Done,
    simplified_matcher::{has_value, lacks_value},
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::RepresentationSpelling},
    model::Entity,
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Retrieve Entity on broker A with attrs speed and options `keyValues` of an Entity broker B serves through
/// an inclusive registration answers speed as the bare value 42 and no `brandName`
/// Clause: 5.7.1.4, 4.5.4, 6.4.3.2
#[gtest]
#[tokio::test]
async fn n_fed_ret_7603_attrs_and_key_values_apply_to_an_entity_served_by_the_peer() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let broker_b = broker::client(BrokerRole::B)?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 42)
        .property("brandName", "Mercedes")
        .build()?;
    broker_b.entities().create(&vehicle).send().await?.done()?;
    let registration = CSourceRegistration::builder(broker::peer(PeerRole::B)?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern_ending("Vehicle", "1")?)])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::RetrieveEntity])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    let retrieved = broker
        .with_accept(Accept::any())
        .entities()
        .retrieve_simplified(&vehicle_id)
        .attrs(vec!["speed".parse()?])
        .spelling(RepresentationSpelling::default().in_options().key_values())
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_value("speed", 42));
    expect_that!(entity, lacks_value("brandName"));
    Ok(())
}
