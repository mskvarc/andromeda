//! One query answers the local entity and the entities of three peers.

use andromeda_test_suite::{
    broker,
    cleanup::Cleanup,
    config::{BrokerRole, PeerRole},
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

/// Pins: Query Entities on broker A by type, holding one Entity itself and inclusive registrations of brokers B, C
/// and D that hold one Entity each, answers all four Entities
/// Clause: 4.5.5, 5.7.2.4
#[gtest]
#[tokio::test]
async fn n_fed_qry_8006_one_query_answers_the_local_entity_and_the_entities_of_three_peers() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let broker_b = broker::client(BrokerRole::B)?;
    let building_b_id = namespace.urn("Building", "b")?;
    let building_type = namespace.entity_type("Building")?;
    let broker_c = broker::client(BrokerRole::C)?;
    let building_c_id = namespace.urn("Building", "c")?;
    let broker_d = broker::client(BrokerRole::D)?;
    let building_d_id = namespace.urn("Building", "d")?;
    let registration_b_id = namespace.registration_id("b")?;
    let registration_c_id = namespace.registration_id("c")?;
    let registration_d_id = namespace.registration_id("d")?;
    let building_a_id = namespace.urn("Building", "a")?;
    let building_b = Entity::builder()
        .id(building_b_id.clone())
        .entity_type(building_type.clone())
        .property("speed", 1)
        .build()?;
    broker_b.entities().create(&building_b).send().await?.done()?;
    let building_c = Entity::builder()
        .id(building_c_id.clone())
        .entity_type(building_type.clone())
        .property("speed", 1)
        .build()?;
    broker_c.entities().create(&building_c).send().await?.done()?;
    let building_d = Entity::builder()
        .id(building_d_id.clone())
        .entity_type(building_type.clone())
        .property("speed", 1)
        .build()?;
    broker_d.entities().create(&building_d).send().await?.done()?;
    let registration_b = CSourceRegistration::builder(broker::peer(PeerRole::B)?)
        .id(registration_b_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(building_type.clone()).with_id_pattern(namespace.id_pattern("Building")?)])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::QueryEntity])
        .build()?;
    broker.registrations().register(&registration_b).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_b_cleanup = Cleanup::of(&broker).registration(&registration_b_id);
    let registration_c = CSourceRegistration::builder(broker::peer(PeerRole::C)?)
        .id(registration_c_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(building_type.clone()).with_id_pattern(namespace.id_pattern("Building")?)])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::QueryEntity])
        .build()?;
    broker.registrations().register(&registration_c).send().await?;
    let _registration_c_cleanup = Cleanup::of(&broker).registration(&registration_c_id);
    let registration_d = CSourceRegistration::builder(broker::peer(PeerRole::D)?)
        .id(registration_d_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(building_type.clone()).with_id_pattern(namespace.id_pattern("Building")?)])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::QueryEntity])
        .build()?;
    broker.registrations().register(&registration_d).send().await?;
    let _registration_d_cleanup = Cleanup::of(&broker).registration(&registration_d_id);
    let building_a = Entity::builder()
        .id(building_a_id.clone())
        .entity_type(building_type.clone())
        .property("speed", 1)
        .build()?;
    broker.entities().create(&building_a).send().await?.done()?;
    let page = broker.with_accept(Accept::any()).entities().query().entity_type(&building_type).send().await?;
    expect_that!(page.items(), len(eq(4)));
    expect_that!(page.items(), contains(has_id(&building_a_id)));
    expect_that!(page.items(), contains(has_id(&building_b_id)));
    expect_that!(page.items(), contains(has_id(&building_c_id)));
    expect_that!(page.items(), contains(has_id(&building_d_id)));
    Ok(())
}
