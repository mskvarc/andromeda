//! A temporal query around a registration cycle of three brokers answers the entity once.

use andromeda_test_suite::{
    broker,
    cleanup::Cleanup,
    config::{BrokerRole, PeerRole},
    evolution_matcher::has_evolution,
    identity_matcher::has_id,
    namespace::Namespace,
    nth::nth,
    outcome::Upserted,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Property, evolution::TemporalEntity},
    query::temporal_query::{self, TemporalProperty},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, TimeInterval},
};

/// Pins: Query Temporal Evolution of Entities on broker A, with `queryTemporal` registrations from A to broker D,
/// from D to broker C and from C back to A, answers the Entity broker C holds exactly once with its two instances
/// Clause: 5.7.4.4, 4.3.6.4, 6.3.18, 5.12
#[gtest]
#[tokio::test]
async fn n_fed_tmp_8001_a_temporal_query_around_a_registration_cycle_of_three_brokers_answers_the_entity_once() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let broker_c = broker::client(BrokerRole::C)?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let broker_d = broker::client(BrokerRole::D)?;
    let registration_d_id = namespace.registration_id("d")?;
    let registration_c_id = namespace.registration_id("c")?;
    let registration_a_id = namespace.registration_id("a")?;
    let building_evolution = TemporalEntity::builder()
        .id(building_id.clone())
        .entity_type(building_type.clone())
        .property("speed", Property::builder(5).observed_at("2024-01-01T00:00:00Z".parse()?))
        .property("speed", Property::builder(6).observed_at("2024-01-02T00:00:00Z".parse()?))
        .build()?;
    broker_c.temporal().create_or_update(&building_evolution).send().await?.created()?;
    let registration_d = CSourceRegistration::builder(broker::peer(PeerRole::C)?)
        .id(registration_d_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(building_type.clone()).with_id_pattern(namespace.id_pattern("Building")?)])
            .build()?])
        .operations([DistributedOperation::QueryTemporal])
        .observation_interval(TimeInterval::between("2023-01-01T00:00:00Z".parse()?, "2025-01-01T00:00:00Z".parse()?))
        .build()?;
    broker_d.registrations().register(&registration_d).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_d_cleanup = Cleanup::of(&broker_d).registration(&registration_d_id);
    let registration_c = CSourceRegistration::builder(broker::peer(PeerRole::A)?)
        .id(registration_c_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(building_type.clone()).with_id_pattern(namespace.id_pattern("Building")?)])
            .build()?])
        .operations([DistributedOperation::QueryTemporal])
        .observation_interval(TimeInterval::between("2023-01-01T00:00:00Z".parse()?, "2025-01-01T00:00:00Z".parse()?))
        .build()?;
    broker_c.registrations().register(&registration_c).send().await?;
    let _registration_c_cleanup = Cleanup::of(&broker_c).registration(&registration_c_id);
    let registration_a = CSourceRegistration::builder(broker::peer(PeerRole::D)?)
        .id(registration_a_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(building_type.clone()).with_id_pattern(namespace.id_pattern("Building")?)])
            .build()?])
        .operations([DistributedOperation::QueryTemporal])
        .observation_interval(TimeInterval::between("2023-01-01T00:00:00Z".parse()?, "2025-01-01T00:00:00Z".parse()?))
        .build()?;
    broker.registrations().register(&registration_a).send().await?;
    let _registration_a_cleanup = Cleanup::of(&broker).registration(&registration_a_id);
    let page = broker
        .with_accept(Accept::any())
        .temporal()
        .query(temporal_query::between("2024-01-01T00:00:00Z".parse()?, "2024-01-03T00:00:00Z".parse()?).time_property(TemporalProperty::ObservedAt))
        .entity_type(&building_type)
        .ids(vec![building_id.clone()])
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, all!(has_id(&building_id), has_evolution("speed", len(eq(2))))));
    Ok(())
}
