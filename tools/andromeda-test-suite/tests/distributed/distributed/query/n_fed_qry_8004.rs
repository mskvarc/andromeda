//! A query reaches an entity held two registrations away.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    cleanup::Cleanup,
    config::{BrokerRole, PeerRole},
    entity_matcher::has_attribute,
    identity_matcher::has_id,
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

/// Pins: Query Entities on broker A by type, under an inclusive registration of broker D, which itself holds an
/// inclusive registration of broker C for the Entity, answers the Entity broker C holds
/// Clause: 4.3.6.4, 5.7.2.4
#[gtest]
#[tokio::test]
async fn n_fed_qry_8004_a_query_reaches_an_entity_held_two_registrations_away() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let broker_c = broker::client(BrokerRole::C)?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let broker_d = broker::client(BrokerRole::D)?;
    let registration_d_id = namespace.registration_id("d")?;
    let registration_a_id = namespace.registration_id("a")?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type.clone())
        .property("speed", 8)
        .build()?;
    broker_c.entities().create(&building).send().await?.done()?;
    let registration_d = CSourceRegistration::builder(broker::peer(PeerRole::C)?)
        .id(registration_d_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(building_type.clone()).with_id(building_id.clone())])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::QueryEntity])
        .build()?;
    broker_d.registrations().register(&registration_d).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_d_cleanup = Cleanup::of(&broker_d).registration(&registration_d_id);
    let registration_a = CSourceRegistration::builder(broker::peer(PeerRole::D)?)
        .id(registration_a_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(building_type.clone()).with_id(building_id.clone())])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::QueryEntity])
        .build()?;
    broker.registrations().register(&registration_a).send().await?;
    let _registration_a_cleanup = Cleanup::of(&broker).registration(&registration_a_id);
    let page = broker.with_accept(Accept::any()).entities().query().entity_type(&building_type).send().await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(
        page.items(),
        nth(0, all!(has_id(&building_id), has_attribute("speed", any_attribute().value(8))))
    );
    Ok(())
}
