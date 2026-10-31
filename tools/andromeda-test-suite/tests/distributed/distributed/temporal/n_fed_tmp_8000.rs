//! A temporal values query merges the pairs of two peers into one object.

use andromeda_test_suite::{
    broker,
    config::{BrokerRole, PeerRole},
    identity_matcher::has_id,
    namespace::Namespace,
    nth::nth,
    outcome::Upserted,
    values_matcher::{has_values, pair},
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Property, evolution::TemporalEntity},
    query::temporal_query::{self, TemporalProperty},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, TimeInterval},
};

/// Pins: Query Temporal Evolution of Entities on broker A as `temporalValues`, over `queryTemporal` registrations
/// of brokers B and C holding two speed instances each of one Entity, answers that Entity once with one speed
/// object whose pairs are the four instances
/// Clause: 5.7.4.4, 4.5.9, 4.5.5
#[gtest]
#[tokio::test]
async fn n_fed_tmp_8000_a_temporal_values_query_merges_the_pairs_of_two_peers_into_one_object() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let broker_b = broker::client(BrokerRole::B)?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let broker_c = broker::client(BrokerRole::C)?;
    let registration_b_id = namespace.registration_id("b")?;
    let registration_c_id = namespace.registration_id("c")?;
    let building_evolution = TemporalEntity::builder()
        .id(building_id.clone())
        .entity_type(building_type.clone())
        .property("speed", Property::builder(1).observed_at("2024-01-01T00:00:00Z".parse()?))
        .property("speed", Property::builder(2).observed_at("2024-01-01T06:00:00Z".parse()?))
        .build()?;
    broker_b.temporal().create_or_update(&building_evolution).send().await?.created()?;
    let building_evolution_2 = TemporalEntity::builder()
        .id(building_id.clone())
        .entity_type(building_type.clone())
        .property("speed", Property::builder(3).observed_at("2024-01-01T12:00:00Z".parse()?))
        .property("speed", Property::builder(4).observed_at("2024-01-01T18:00:00Z".parse()?))
        .build()?;
    broker_c.temporal().create_or_update(&building_evolution_2).send().await?.created()?;
    let registration_b = CSourceRegistration::builder(broker::peer(PeerRole::B)?)
        .id(registration_b_id)
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(building_type.clone()).with_id_pattern(namespace.id_pattern("Building")?)])
            .build()?])
        .operations([DistributedOperation::QueryTemporal])
        .observation_interval(TimeInterval::between("2023-01-01T00:00:00Z".parse()?, "2025-01-01T00:00:00Z".parse()?))
        .build()?;
    broker.registrations().register(&registration_b).send().await?;
    let registration_c = CSourceRegistration::builder(broker::peer(PeerRole::C)?)
        .id(registration_c_id)
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(building_type.clone()).with_id_pattern(namespace.id_pattern("Building")?)])
            .build()?])
        .operations([DistributedOperation::QueryTemporal])
        .observation_interval(TimeInterval::between("2023-01-01T00:00:00Z".parse()?, "2025-01-01T00:00:00Z".parse()?))
        .build()?;
    broker.registrations().register(&registration_c).send().await?;
    let page = broker
        .with_accept(Accept::any())
        .temporal()
        .query_values(temporal_query::between("2024-01-01T00:00:00Z".parse()?, "2024-01-03T00:00:00Z".parse()?).time_property(TemporalProperty::ObservedAt))
        .entity_type(&building_type)
        .ids(vec![building_id.clone()])
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(
        page.items(),
        nth(
            0,
            all!(
                has_id(&building_id),
                has_values(
                    "speed",
                    all!(
                        len(eq(4)),
                        contains(pair().value(1).time("2024-01-01T00:00:00Z".parse()?)),
                        contains(pair().value(2).time("2024-01-01T06:00:00Z".parse()?)),
                        contains(pair().value(3).time("2024-01-01T12:00:00Z".parse()?)),
                        contains(pair().value(4).time("2024-01-01T18:00:00Z".parse()?))
                    )
                )
            )
        )
    );
    Ok(())
}
