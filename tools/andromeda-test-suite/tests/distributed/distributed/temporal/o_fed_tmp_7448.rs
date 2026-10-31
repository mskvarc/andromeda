//! An aggregated split query keeps the entity the peer instance in effect admits.

use andromeda_test_suite::{
    aggregation_matcher::{has_aggregate, period},
    broker,
    cleanup::Cleanup,
    config::{BrokerRole, PeerRole},
    namespace::Namespace,
    nth::nth,
    outcome::Upserted,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{
        NgsiLdMediaType,
        Property,
        evolution::{AggregationMethod, TemporalEntity},
    },
    query::{EntitySplitting, q, temporal_query},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo},
};

/// Pins: under `splitEntities` an `aggregatedValues` query on broker A with `aggrMethods` max and q=speed>50
/// answers a Vehicle whose speed broker A observed 30 at 12:10 and broker B 60 at 11:00, aggregated over the
/// window's instances alone: max 30
/// Clause: 5.7.4.4, 4.11, 4.5.19
#[gtest]
#[tokio::test]
async fn o_fed_tmp_7448_an_aggregated_split_query_keeps_the_entity_the_peer_instance_in_effect_admits() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let broker_b = broker::client(BrokerRole::B)?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_evolution = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(30).observed_at("2020-08-01T12:10:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&vehicle_evolution).send().await?.created()?;
    let vehicle_evolution_2 = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(60).observed_at("2020-08-01T11:00:00Z".parse()?))
        .build()?;
    broker_b.temporal().create_or_update(&vehicle_evolution_2).send().await?.created()?;
    let registration = CSourceRegistration::builder(broker::peer(PeerRole::B)?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .operations([DistributedOperation::QueryTemporal])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query_aggregated(
            temporal_query::between("2020-08-01T12:00:00Z".parse()?, "2020-08-01T13:00:00Z".parse()?).aggregation_methods([AggregationMethod::Max]),
        )
        .entity_type(&vehicle_type)
        .split_entities(EntitySplitting::Split)
        .q(q::attr("speed").gt(50))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    let aggregated = page.items().first().or_fail()?;
    expect_that!(aggregated.id, eq(&vehicle_id));
    expect_that!(aggregated, has_aggregate("speed", AggregationMethod::Max, nth(0, period().value(30))));
    Ok(())
}
