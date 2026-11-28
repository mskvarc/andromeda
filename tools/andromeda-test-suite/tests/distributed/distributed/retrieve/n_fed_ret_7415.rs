//! Two sources conflicting without observed at answer the one modified later in any registration order.

use andromeda_test_suite::{
    attribute_matcher::property,
    broker,
    cleanup::Cleanup,
    config::BrokerRole,
    entity_matcher::{has_instance, has_instances},
    namespace::Namespace,
    outcome::Done,
    peer_role::PeerRole,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, Property},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo},
};

/// Pins: Retrieve Entity on broker A, which holds neither Vehicle, answers for Vehicle 1 the speed 30 broker B wrote
/// after broker C wrote 20, and for Vehicle 2 the speed 50 broker C wrote after broker B wrote 40, though broker B's
/// registration identifier orders before broker C's for both
/// Clause: 5.7.1.4, 4.5.5.3, ETSI TS 104 175 V1.1.1 clause 8.5.3
#[gtest]
#[tokio::test]
async fn n_fed_ret_7415_two_sources_conflicting_without_observed_at_answer_the_one_modified_later_in_any_registration_order() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let broker_b = broker::client(BrokerRole::B)?;
    let broker_c = broker::client(BrokerRole::C)?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_2_id = namespace.urn("Vehicle", 2)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration_id = namespace.registration_id(1)?;
    let registration_2_id = namespace.registration_id(2)?;
    let speed_of = |id, speed: i64| {
        Entity::builder()
            .id(id)
            .entity_type(vehicle_type.clone())
            .property("speed", Property::builder(speed))
            .build()
    };
    broker_c.entities().create(&speed_of(vehicle_id.clone(), 20)?).send().await?.done()?;
    broker_b.entities().create(&speed_of(vehicle_id.clone(), 30)?).send().await?.done()?;
    broker_b.entities().create(&speed_of(vehicle_2_id.clone(), 40)?).send().await?.done()?;
    broker_c.entities().create(&speed_of(vehicle_2_id.clone(), 50)?).send().await?.done()?;
    let registration = CSourceRegistration::builder(broker::peer(PeerRole::B)?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .property_names(["speed"])
            .build()?])
        .operations([DistributedOperation::RetrieveEntity])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    let registration_2 = CSourceRegistration::builder(broker::peer(PeerRole::C)?)
        .id(registration_2_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .property_names(["speed"])
            .build()?])
        .operations([DistributedOperation::RetrieveEntity])
        .build()?;
    broker.registrations().register(&registration_2).send().await?;
    let _registration_2_cleanup = Cleanup::of(&broker).registration(&registration_2_id);
    let retrieved = broker.with_accept(Accept::any()).entities().retrieve(&vehicle_id).send().await?;
    expect_that!(retrieved.body(), has_instances("speed", 1));
    expect_that!(retrieved.body(), has_instance("speed", property().value(30)));
    let retrieved_2 = broker.with_accept(Accept::any()).entities().retrieve(&vehicle_2_id).send().await?;
    expect_that!(retrieved_2.body(), has_instances("speed", 1));
    expect_that!(retrieved_2.body(), has_instance("speed", property().value(50)));
    Ok(())
}
