//! A peer relationship reaches a linked entity the front broker holds.

use andromeda_test_suite::{
    body_json::{json_count, json_equals},
    broker,
    config::{BrokerRole, PeerRole},
    namespace::Namespace,
    outcome::Upserted,
    problem::answered,
    query_join::with_join,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{
        headers::Accept,
        operation::{NormalizedTemporal, QueryTemporalEvolution},
    },
    model::{NgsiLdMediaType, Property, Relationship, evolution::TemporalEntity},
    query::{EntitySplitting, Join, builders::SelectorTypeBuilder, q, temporal_query},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo},
};

/// Pins: under `splitEntities` and `temporal_linked_retrieval` a between 12:00-13:00 query on broker E answers
/// `q=parkedIn{free}>0` for Vehicle 1, whose `parkedIn` broker B observed at 12:10 naming Parking 1, which broker E
/// holds free 3 at 12:20, and not for Vehicle 2, whose Parking 2 broker E holds free 0
/// Clause: 5.7.4.4, 4.9, 4.5.23, 4.11
/// Recorded: Andromeda extension, not v1.9.1: clause 5.7.4.4 raises `BadRequestData` when filter conditions
/// indicate the use of Linked Entity retrieval; the `temporal_linked_retrieval` extension, on broker E alone,
/// admits the hop under the Core rules over the window of the request
#[gtest]
#[tokio::test]
async fn a_fed_tmp_7451_a_peer_relationship_reaches_a_linked_entity_the_front_broker_holds() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker_e = broker::client(BrokerRole::E)?;
    let broker_b = broker::client(BrokerRole::B)?;
    let parking_id = namespace.urn("Parking", 1)?;
    let parking_type = namespace.entity_type("Parking")?;
    let parking_2_id = namespace.urn("Parking", 2)?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_2_id = namespace.urn("Vehicle", 2)?;
    let registration_id = namespace.registration_id(1)?;
    let parking = TemporalEntity::builder()
        .id(parking_id.clone())
        .entity_type(parking_type.clone())
        .property("free", Property::builder(3).observed_at("2020-08-01T12:20:00Z".parse()?))
        .build()?;
    broker_e.temporal().create_or_update(&parking).send().await?.created()?;
    let parking_2 = TemporalEntity::builder()
        .id(parking_2_id.clone())
        .entity_type(parking_type.clone())
        .property("free", Property::builder(0).observed_at("2020-08-01T12:20:00Z".parse()?))
        .build()?;
    broker_e.temporal().create_or_update(&parking_2).send().await?.created()?;
    let vehicle = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(60).observed_at("2020-08-01T12:10:00Z".parse()?))
        .build()?;
    broker_e.temporal().create_or_update(&vehicle).send().await?.created()?;
    let vehicle_2 = TemporalEntity::builder()
        .id(vehicle_2_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(60).observed_at("2020-08-01T12:10:00Z".parse()?))
        .build()?;
    broker_e.temporal().create_or_update(&vehicle_2).send().await?.created()?;
    let vehicle_b = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .relationship(
            "parkedIn",
            Relationship::builder(parking_id.clone()).observed_at("2020-08-01T12:10:00Z".parse()?),
        )
        .build()?;
    broker_b.temporal().create_or_update(&vehicle_b).send().await?.created()?;
    let vehicle_2_b = TemporalEntity::builder()
        .id(vehicle_2_id.clone())
        .entity_type(vehicle_type.clone())
        .relationship(
            "parkedIn",
            Relationship::builder(parking_2_id.clone()).observed_at("2020-08-01T12:10:00Z".parse()?),
        )
        .build()?;
    broker_b.temporal().create_or_update(&vehicle_2_b).send().await?.created()?;
    let registration = CSourceRegistration::builder(broker::peer(PeerRole::B)?)
        .id(registration_id)
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .operations([DistributedOperation::QueryTemporal])
        .build()?;
    broker_e.registrations().register(&registration).send().await?;
    let temporal_q = temporal_query::between("2020-08-01T12:00:00Z".parse()?, "2020-08-01T13:00:00Z".parse()?).build()?;
    let selector = SelectorTypeBuilder::from(&vehicle_type).build()?;
    let linked_q = q::attr("parkedIn").linked("free").gt(0).build()?;
    let query = QueryTemporalEvolution::builder()
        .temporal_q(&temporal_q)
        .entity_type(&selector)
        .q(&linked_q)
        .split_entities(EntitySplitting::Split)
        .format(NormalizedTemporal)
        .build();
    // The temporal query writes no `join`, so the parameters follow the query the operation writes. Each vehicle nests
    // its linked parking, which no model reads, so the answer is read as received.
    let request = with_join(broker_e.with_accept(Accept::new(NgsiLdMediaType::Json)).unchecked(&query)?, Join::Inline, 1)?;
    let answer = broker_e.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::OK));
    let response = answer?;
    expect_that!(response.body(), json_count("$", 1));
    expect_that!(response.body(), json_equals("$[0].id", vehicle_id.to_string()));
    Ok(())
}
