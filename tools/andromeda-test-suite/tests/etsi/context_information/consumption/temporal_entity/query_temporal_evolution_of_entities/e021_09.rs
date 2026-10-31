//! The temporal evolutions of the Vehicles are queried by a geoquery, a Vehicle near a point and a Vehicle within a
//! polygon (clause 5.7.4). Only the Vehicle with a location is answered, with every instance of its attributes and of
//! its location.
//!
//! The answers are compared exactly with the test's expectation.

use crate::{
    geometry_payloads::{point, polygon},
    temporal_operations::{create_temporal_entity, delete_temporal_entities},
    vehicle_evolution_payloads::{VEHICLE_SPEED_FUEL_TERMS, vehicle_speed_fuel_level_on},
};
use andromeda_test_suite::{
    broker::{self, BrokerClient},
    comparison::matches_document,
    namespace::Namespace,
    problem::answered,
    vocabulary::Vocabulary,
    wire::json_body,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{GeoProperty, Input, Property, evolution::TemporalEntity},
    query::{geoquery, temporal_query},
};
use serde_json::{Value, json};

/// The Vehicle `urn:ngsi-ld:Vehicle:<ns>-021-09-B` of the case: a `speed` of 150, 90 and 127 and a `fuelLevel` of
/// 89, 78
/// and 62, observed on 2020-10-01 at 12:03, 12:05 and 12:07 and at 12:03, 13:05 and 14:07 in UTC, and a `location` at
/// 2.35, 42.22 observed at 12:03 and at -8.5, 41.2 observed at 12:05.
fn vehicle_located_in_october(namespace: &Namespace) -> Result<TemporalEntity<Input>> {
    Ok(TemporalEntity::builder()
        .id(namespace.urn("Vehicle", "021-09-B")?)
        .entity_type("Vehicle")
        .property("speed", Property::builder(150).observed_at("2020-10-01T12:03:00Z".parse()?))
        .property("speed", Property::builder(90).observed_at("2020-10-01T12:05:00Z".parse()?))
        .property("speed", Property::builder(127).observed_at("2020-10-01T12:07:00Z".parse()?))
        .property("fuelLevel", Property::builder(89).observed_at("2020-10-01T12:03:00Z".parse()?))
        .property("fuelLevel", Property::builder(78).observed_at("2020-10-01T13:05:00Z".parse()?))
        .property("fuelLevel", Property::builder(62).observed_at("2020-10-01T14:07:00Z".parse()?))
        .geo_property(
            "location",
            GeoProperty::builder(point(2.35, 42.22)?).observed_at("2020-10-01T12:03:00Z".parse()?),
        )
        .geo_property(
            "location",
            GeoProperty::builder(point(-8.5, 41.2)?).observed_at("2020-10-01T12:05:00Z".parse()?),
        )
        .build()?)
}

/// Registers the terms of the cases and creates Vehicle `021-09-A`, observed on 2020-08-01 without a location, and
/// Vehicle `021-09-B` of [`vehicle_located_in_october`], in the vocabulary's context.
async fn create_vehicles(broker: &BrokerClient, namespace: &Namespace) -> Result<Vocabulary> {
    let vocabulary = Vocabulary::register(broker, namespace, &VEHICLE_SPEED_FUEL_TERMS).await?;
    create_temporal_entity(broker, &vocabulary, &vehicle_speed_fuel_level_on(namespace, "021-09-A", "2020-08-01")?).await?;
    create_temporal_entity(broker, &vocabulary, &vehicle_located_in_october(namespace)?).await?;
    Ok(vocabulary)
}

/// The expected answer of the geoquery cases: the one Vehicle with a location, with every instance it holds.
fn located_vehicle(namespace: &Namespace) -> Value {
    json!({
        "id": namespace.urn_text("Vehicle", "021-09-B"),
        "type": "Vehicle",
        "speed": [
            {"type": "Property", "value": 150, "observedAt": "2020-10-01T12:03:00Z"},
            {"type": "Property", "value": 90, "observedAt": "2020-10-01T12:05:00Z"},
            {"type": "Property", "value": 127, "observedAt": "2020-10-01T12:07:00Z"},
        ],
        "fuelLevel": [
            {"type": "Property", "value": 89, "observedAt": "2020-10-01T12:03:00Z"},
            {"type": "Property", "value": 78, "observedAt": "2020-10-01T13:05:00Z"},
            {"type": "Property", "value": 62, "observedAt": "2020-10-01T14:07:00Z"},
        ],
        "location": [
            {
                "type": "GeoProperty",
                "value": {"type": "Point", "coordinates": [2.35, 42.22]},
                "observedAt": "2020-10-01T12:03:00Z",
            },
            {
                "type": "GeoProperty",
                "value": {"type": "Point", "coordinates": [-8.5, 41.2]},
                "observedAt": "2020-10-01T12:05:00Z",
            },
        ],
    })
}

/// Pins: Check that one can query the temporal evolution of entities matching the given NGSI-LD geo-query
/// Clause: 5.7.4
#[gtest]
#[tokio::test]
async fn e021_09_01_near_point() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_vehicles(&broker, &namespace).await?;

    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
        .with_responses()
        .temporal()
        .query(temporal_query::after("2020-07-01T12:05:00Z".parse()?))
        .entity_type("Vehicle")
        .geo_q(geoquery::near_max(2000, point(-8.503, 41.202)?))
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    expect_that!(&body, matches_document(json!([located_vehicle(&namespace)])).hiding("instanceId"));

    delete_temporal_entities(&broker, &[&namespace.urn("Vehicle", "021-09-A")?, &namespace.urn("Vehicle", "021-09-B")?]).await;
    Ok(())
}

/// Pins: Check that one can query the temporal evolution of entities matching the given NGSI-LD geo-query
/// Clause: 5.7.4
#[gtest]
#[tokio::test]
async fn e021_09_02_within_polygon() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_vehicles(&broker, &namespace).await?;

    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
        .with_responses()
        .temporal()
        .query(temporal_query::after("2020-07-01T12:05:00Z".parse()?))
        .entity_type("Vehicle")
        .geo_q(
            geoquery::within(polygon(&[
                (-13.503, 47.202),
                (6.541, 52.961),
                (20.37, 44.653),
                (9.46, 32.57),
                (-13.503, 32.57),
                (-13.503, 47.202),
            ])?)
            .geoproperty("location"),
        )
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    expect_that!(&body, matches_document(json!([located_vehicle(&namespace)])).hiding("instanceId"));

    delete_temporal_entities(&broker, &[&namespace.urn("Vehicle", "021-09-A")?, &namespace.urn("Vehicle", "021-09-B")?]).await;
    Ok(())
}
