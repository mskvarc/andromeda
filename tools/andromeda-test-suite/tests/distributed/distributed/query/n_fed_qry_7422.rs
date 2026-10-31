//! An entity only an auxiliary peer holds is part of the answer.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    cleanup::Cleanup,
    config::{BrokerRole, PeerRole},
    entity_matcher::{has_attribute, has_instances},
    identity_matcher::has_id,
    namespace::Namespace,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::Entity,
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Query Entities by type on broker A, holding Vehicle 1 with speed 1, under an auxiliary registration of
/// broker B holding Vehicle 1 with speed 99 and fuelLevel 70 and Vehicle 2 with speed 2, answers 200 with Vehicle 1
/// carrying A's speed 1 and B's fuelLevel 70, and Vehicle 2 carrying speed 2
/// Clause: 5.7.2.4, 4.3.6.2
#[gtest]
#[tokio::test]
async fn n_fed_qry_7422_an_entity_only_an_auxiliary_peer_holds_is_part_of_the_answer() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_2_id = namespace.urn("Vehicle", 2)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let broker_b = broker::client(BrokerRole::B)?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 1)
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let held_by_b = [
        Entity::builder()
            .id(vehicle_id.clone())
            .entity_type(vehicle_type.clone())
            .property("speed", 99)
            .property("fuelLevel", 70)
            .build()?,
        Entity::builder()
            .id(vehicle_2_id.clone())
            .entity_type(vehicle_type.clone())
            .property("speed", 2)
            .build()?,
    ];
    broker_b.entities().batch_create(&held_by_b).send().await?.done()?;
    let registration = CSourceRegistration::builder(broker::peer(PeerRole::B)?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder().entities([EntityInfo::new(vehicle_type.clone())]).build()?])
        .mode(RegistrationMode::Auxiliary)
        .operations([DistributedOperation::QueryEntity])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    let page = broker.with_accept(Accept::any()).entities().query().entity_type(&vehicle_type).send().await?;
    expect_that!(page.items(), len(eq(2)));
    expect_that!(
        page.items(),
        contains(all!(
            has_id(&vehicle_id),
            has_instances("speed", 1),
            has_attribute("speed", any_attribute().value(1)),
            has_attribute("fuelLevel", any_attribute().value(70))
        ))
    );
    expect_that!(
        page.items(),
        contains(all!(has_id(&vehicle_2_id), has_attribute("speed", any_attribute().value(2))))
    );
    Ok(())
}
