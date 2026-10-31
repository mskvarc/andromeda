//! Create routes each entity to the redirect source its id pattern names.

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
    query::Locality,
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, OperationGroup, RegistrationInfo, RegistrationMode, SupportedOperation},
};

/// Pins: Batch Entity Creation on broker A of two Entities for each of three redirect registrations of brokers B, C
/// and D, told apart by `idPattern`, answers 201 with the six ids, and each broker holds its own two Entities and
/// none of the others, while broker A holds none
/// Clause: 5.6.7.4, 4.3.6.3, 5.12
#[gtest]
#[tokio::test]
async fn n_fed_bat_8000_create_routes_each_entity_to_the_redirect_source_its_id_pattern_names() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let registration_mad_id = namespace.registration_id("mad")?;
    let building_type = namespace.entity_type("Building")?;
    let registration_ber_id = namespace.registration_id("ber")?;
    let registration_nor_id = namespace.registration_id("nor")?;
    let building_mad_1_id = namespace.urn("Building", "mad-1")?;
    let building_mad_2_id = namespace.urn("Building", "mad-2")?;
    let building_ber_1_id = namespace.urn("Building", "ber-1")?;
    let building_ber_2_id = namespace.urn("Building", "ber-2")?;
    let building_nor_1_id = namespace.urn("Building", "nor-1")?;
    let building_nor_2_id = namespace.urn("Building", "nor-2")?;
    let broker_b = broker::client(BrokerRole::B)?;
    let broker_c = broker::client(BrokerRole::C)?;
    let broker_d = broker::client(BrokerRole::D)?;
    let registration_mad = CSourceRegistration::builder(broker::peer(PeerRole::B)?)
        .id(registration_mad_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(building_type.clone()).with_id_pattern(namespace.id_pattern_ending("Building", "mad-.*")?)])
            .build()?])
        .mode(RegistrationMode::Redirect)
        .operations([
            SupportedOperation::from(OperationGroup::RedirectionOps),
            SupportedOperation::from(DistributedOperation::CreateBatch),
        ])
        .build()?;
    broker.registrations().register(&registration_mad).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_mad_cleanup = Cleanup::of(&broker).registration(&registration_mad_id);
    let registration_ber = CSourceRegistration::builder(broker::peer(PeerRole::C)?)
        .id(registration_ber_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(building_type.clone()).with_id_pattern(namespace.id_pattern_ending("Building", "ber-.*")?)])
            .build()?])
        .mode(RegistrationMode::Redirect)
        .operations([
            SupportedOperation::from(OperationGroup::RedirectionOps),
            SupportedOperation::from(DistributedOperation::CreateBatch),
        ])
        .build()?;
    broker.registrations().register(&registration_ber).send().await?;
    let _registration_ber_cleanup = Cleanup::of(&broker).registration(&registration_ber_id);
    let registration_nor = CSourceRegistration::builder(broker::peer(PeerRole::D)?)
        .id(registration_nor_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(building_type.clone()).with_id_pattern(namespace.id_pattern_ending("Building", "nor-.*")?)])
            .build()?])
        .mode(RegistrationMode::Redirect)
        .operations([
            SupportedOperation::from(OperationGroup::RedirectionOps),
            SupportedOperation::from(DistributedOperation::CreateBatch),
        ])
        .build()?;
    broker.registrations().register(&registration_nor).send().await?;
    let _registration_nor_cleanup = Cleanup::of(&broker).registration(&registration_nor_id);
    let entities = [
        Entity::builder()
            .id(building_mad_1_id.clone())
            .entity_type(building_type.clone())
            .property("speed", 1)
            .build()?,
        Entity::builder()
            .id(building_mad_2_id.clone())
            .entity_type(building_type.clone())
            .property("speed", 2)
            .build()?,
        Entity::builder()
            .id(building_ber_1_id.clone())
            .entity_type(building_type.clone())
            .property("speed", 1)
            .build()?,
        Entity::builder()
            .id(building_ber_2_id.clone())
            .entity_type(building_type.clone())
            .property("speed", 2)
            .build()?,
        Entity::builder()
            .id(building_nor_1_id.clone())
            .entity_type(building_type.clone())
            .property("speed", 1)
            .build()?,
        Entity::builder()
            .id(building_nor_2_id.clone())
            .entity_type(building_type.clone())
            .property("speed", 2)
            .build()?,
    ];
    let created = broker.entities().batch_create(&entities).send().await?.done()?;
    expect_that!(created, len(eq(6)));
    expect_that!(created, contains(eq(&building_mad_1_id)));
    expect_that!(created, contains(eq(&building_mad_2_id)));
    expect_that!(created, contains(eq(&building_ber_1_id)));
    expect_that!(created, contains(eq(&building_ber_2_id)));
    expect_that!(created, contains(eq(&building_nor_1_id)));
    expect_that!(created, contains(eq(&building_nor_2_id)));
    let page = broker_b
        .with_accept(Accept::any())
        .entities()
        .query()
        .entity_type(&building_type)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(2)));
    expect_that!(page.items(), contains(has_id(&building_mad_1_id)));
    expect_that!(page.items(), contains(has_id(&building_mad_2_id)));
    let page_2 = broker_c
        .with_accept(Accept::any())
        .entities()
        .query()
        .entity_type(&building_type)
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(2)));
    expect_that!(page_2.items(), contains(has_id(&building_ber_1_id)));
    expect_that!(page_2.items(), contains(has_id(&building_ber_2_id)));
    let page_3 = broker_d
        .with_accept(Accept::any())
        .entities()
        .query()
        .entity_type(&building_type)
        .send()
        .await?;
    expect_that!(page_3.items(), len(eq(2)));
    expect_that!(page_3.items(), contains(has_id(&building_nor_1_id)));
    expect_that!(page_3.items(), contains(has_id(&building_nor_2_id)));
    let page_4 = broker
        .with_accept(Accept::any())
        .entities()
        .query()
        .entity_type(&building_type)
        .locality(Locality::Local)
        .send()
        .await?;
    expect_that!(page_4.items(), len(eq(0)));
    Ok(())
}
