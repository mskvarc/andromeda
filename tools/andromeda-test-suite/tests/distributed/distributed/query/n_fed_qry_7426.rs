//! Two sources conflicting without observed at answer a query with the one modified later in any registration order.

use andromeda_test_suite::{
    attribute_matcher::property,
    broker,
    cleanup::Cleanup,
    config::BrokerRole,
    entity_lookup::with_id,
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

/// Pins: Query Entities by type on broker A, which holds neither Vehicle, answers Vehicle 1 with the speed 30 broker
/// B wrote after broker C wrote 20, and Vehicle 2 with the speed 50 broker C wrote after broker B wrote 40, though
/// broker B's registration identifier orders before broker C's for both
/// Clause: 5.7.2.4, 4.5.5.3, ETSI TS 104 175 V1.1.1 clause 8.5.3
#[gtest]
#[tokio::test]
async fn n_fed_qry_7426_two_sources_conflicting_without_observed_at_answer_a_query_with_the_one_modified_later_in_any_registration_order() -> Result<()> {
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
        .operations([DistributedOperation::QueryEntity])
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
        .operations([DistributedOperation::QueryEntity])
        .build()?;
    broker.registrations().register(&registration_2).send().await?;
    let _registration_2_cleanup = Cleanup::of(&broker).registration(&registration_2_id);
    let page = broker.with_accept(Accept::any()).entities().query().entity_type(&vehicle_type).send().await?;
    expect_that!(page.items(), len(eq(2)));
    let vehicle = with_id(page.items(), &vehicle_id).or_fail()?;
    expect_that!(vehicle, has_instances("speed", 1));
    expect_that!(vehicle, has_instance("speed", property().value(30)));
    let vehicle_2 = with_id(page.items(), &vehicle_2_id).or_fail()?;
    expect_that!(vehicle_2, has_instances("speed", 1));
    expect_that!(vehicle_2, has_instance("speed", property().value(50)));
    Ok(())
}
