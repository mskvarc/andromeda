//! An inclusive peer keeps its attribute over an auxiliary peer when nothing is held locally.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    cleanup::Cleanup,
    config::{BrokerRole, PeerRole},
    entity_matcher::{has_attribute, has_instances},
    namespace::Namespace,
    nth::nth,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::Entity,
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Query Entities by type on broker A, holding nothing of the type, under an inclusive registration of broker
/// B holding Vehicle 1 with speed 10 and an auxiliary registration of broker C holding Vehicle 1 with speed 99 and
/// fuelLevel 70, answers 200 with one Vehicle 1 carrying B's speed 10 alone and C's fuelLevel 70
/// Clause: 5.7.2.4, 4.3.6.2
#[gtest]
#[tokio::test]
async fn n_fed_qry_7423_an_inclusive_peer_keeps_its_attribute_over_an_auxiliary_peer_when_nothing_is_held_locally() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let broker_b = broker::client(BrokerRole::B)?;
    let broker_c = broker::client(BrokerRole::C)?;
    let inclusive_id = namespace.registration_id(1)?;
    let auxiliary_id = namespace.registration_id(2)?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 10)
        .build()?;
    broker_b.entities().create(&vehicle).send().await?.done()?;
    let vehicle_2 = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 99)
        .property("fuelLevel", 70)
        .build()?;
    broker_c.entities().create(&vehicle_2).send().await?.done()?;
    let inclusive = CSourceRegistration::builder(broker::peer(PeerRole::B)?)
        .id(inclusive_id.clone())
        .information([RegistrationInfo::builder().entities([EntityInfo::new(vehicle_type.clone())]).build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::QueryEntity])
        .build()?;
    broker.registrations().register(&inclusive).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _inclusive_cleanup = Cleanup::of(&broker).registration(&inclusive_id);
    let auxiliary = CSourceRegistration::builder(broker::peer(PeerRole::C)?)
        .id(auxiliary_id.clone())
        .information([RegistrationInfo::builder().entities([EntityInfo::new(vehicle_type.clone())]).build()?])
        .mode(RegistrationMode::Auxiliary)
        .operations([DistributedOperation::QueryEntity])
        .build()?;
    broker.registrations().register(&auxiliary).send().await?;
    let _auxiliary_cleanup = Cleanup::of(&broker).registration(&auxiliary_id);
    let page = broker.with_accept(Accept::any()).entities().query().entity_type(&vehicle_type).send().await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(
        page.items(),
        nth(
            0,
            all!(
                has_instances("speed", 1),
                has_attribute("speed", any_attribute().value(10)),
                has_attribute("fuelLevel", any_attribute().value(70))
            )
        )
    );
    Ok(())
}
