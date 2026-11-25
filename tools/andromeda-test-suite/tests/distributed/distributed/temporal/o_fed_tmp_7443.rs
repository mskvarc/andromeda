//! A local value observed before a window opening after now matches in no broker.

use andromeda_test_suite::{broker, cleanup::Cleanup, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Upserted, peer_role::PeerRole};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{NgsiLdMediaType, Property, evolution::TemporalEntity},
    query::{EntitySplitting, q, temporal_query},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo},
};

/// Pins: under `splitEntities` a query on broker A over a window opening after now does not answer q=speed>50 for a
/// Vehicle whose speed 60 broker A observed in 2020 only, and a query over a window holding that instant answers it
/// Clause: 5.7.4.4, 4.11, 4.9
#[gtest]
#[tokio::test]
async fn o_fed_tmp_7443_a_local_value_observed_before_a_window_opening_after_now_matches_nothing() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_evolution = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(60).observed_at("2020-08-01T11:00:00Z".parse()?))
        .property("brand", Property::builder("Mercedes").observed_at("2099-01-01T00:10:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&vehicle_evolution).send().await?.created()?;
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
        .query(temporal_query::between("2099-01-01T00:00:00Z".parse()?, "2099-01-01T01:00:00Z".parse()?))
        .entity_type(&vehicle_type)
        .split_entities(EntitySplitting::Split)
        .q(q::attr("speed").gt(50))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(0)));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::between("2020-08-01T10:00:00Z".parse()?, "2020-08-01T12:00:00Z".parse()?))
        .entity_type(&vehicle_type)
        .split_entities(EntitySplitting::Split)
        .q(q::attr("speed").gt(50))
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(1)));
    expect_that!(page_2.items(), nth(0, has_id(&vehicle_id)));
    Ok(())
}
