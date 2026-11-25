//! A query term reads the value a peer held inside the window, not one it held before.

use andromeda_test_suite::{
    broker,
    cleanup::Cleanup,
    config::BrokerRole,
    identity_matcher::has_id,
    namespace::Namespace,
    nth::nth,
    outcome::Upserted,
    peer_role::PeerRole,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{NgsiLdMediaType, Property, evolution::TemporalEntity},
    query::{EntitySplitting, q, temporal_query},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo},
};

/// Pins: under `splitEntities` a between 12:00-13:00 query on broker A answers q=speed>50 and not q=speed>70 for the
/// Vehicle holding brand on broker A at 12:10 whose speed 60 broker B observed at 12:20, and neither for the Vehicle
/// holding brand on broker A at 12:10 whose speed 60 broker B observed at 11:00 and never since
/// Clause: 5.7.4.4, 4.11, 4.9
#[gtest]
#[tokio::test]
async fn o_fed_tmp_7442_a_query_term_reads_the_value_a_peer_held_inside_the_window_alone() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_2_id = namespace.urn("Vehicle", 2)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let broker_b = broker::client(BrokerRole::B)?;
    let registration_id = namespace.registration_id(1)?;
    for id in [&vehicle_id, &vehicle_2_id] {
        let vehicle_evolution = TemporalEntity::builder()
            .id(id.clone())
            .entity_type(vehicle_type.clone())
            .property("brand", Property::builder("Mercedes").observed_at("2020-08-01T12:10:00Z".parse()?))
            .build()?;
        broker.temporal().create_or_update(&vehicle_evolution).send().await?.created()?;
    }
    for (id, observed_at) in [(&vehicle_id, "2020-08-01T11:00:00Z"), (&vehicle_2_id, "2020-08-01T12:20:00Z")] {
        let peer_evolution = TemporalEntity::builder()
            .id(id.clone())
            .entity_type(vehicle_type.clone())
            .property("speed", Property::builder(60).observed_at(observed_at.parse()?))
            .build()?;
        broker_b.temporal().create_or_update(&peer_evolution).send().await?.created()?;
    }
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
        .query(temporal_query::between("2020-08-01T12:00:00Z".parse()?, "2020-08-01T13:00:00Z".parse()?))
        .entity_type(&vehicle_type)
        .split_entities(EntitySplitting::Split)
        .q(q::attr("speed").gt(50))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&vehicle_2_id)));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::between("2020-08-01T12:00:00Z".parse()?, "2020-08-01T13:00:00Z".parse()?))
        .entity_type(&vehicle_type)
        .split_entities(EntitySplitting::Split)
        .q(q::attr("speed").gt(70))
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(0)));
    Ok(())
}
