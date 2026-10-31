//! One entity held by two peers under distinct dataset ids is answered with both instances.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    cleanup::Cleanup,
    config::{BrokerRole, PeerRole},
    entity_matcher::{has_instance, has_instances},
    identity_matcher::has_id,
    namespace::Namespace,
    nth::nth,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, Property},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Query Entities on broker A by type over inclusive registrations of brokers B and C, which hold one Entity
/// with speed under `datasetId` b and under `datasetId` c respectively, answers that Entity once with both speed
/// instances
/// Clause: 4.5.5, 5.7.2.4
#[gtest]
#[tokio::test]
async fn n_fed_qry_8003_one_entity_held_by_two_peers_under_distinct_dataset_ids_is_answered_with_both_instances() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let broker_b = broker::client(BrokerRole::B)?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let broker_c = broker::client(BrokerRole::C)?;
    let registration_b_id = namespace.registration_id("b")?;
    let registration_c_id = namespace.registration_id("c")?;
    let b_dataset = namespace.dataset_id("Dataset", "b")?;
    let c_dataset = namespace.dataset_id("Dataset", "c")?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type.clone())
        .property("speed", Property::builder(1).dataset_id(namespace.dataset_id("Dataset", "b")?))
        .build()?;
    broker_b.entities().create(&building).send().await?.done()?;
    let building_2 = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type.clone())
        .property("speed", Property::builder(2).dataset_id(namespace.dataset_id("Dataset", "c")?))
        .build()?;
    broker_c.entities().create(&building_2).send().await?.done()?;
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
    let page = broker.with_accept(Accept::any()).entities().query().entity_type(&building_type).send().await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_instances("speed", 2)));
    let item = page.items().first().or_fail()?;
    expect_that!(item, has_instance("speed", any_attribute().value(1).with_dataset_id(b_dataset)));
    expect_that!(item, has_instance("speed", any_attribute().value(2).with_dataset_id(c_dataset)));
    expect_that!(page.items(), nth(0, has_id(&building_id)));
    Ok(())
}
