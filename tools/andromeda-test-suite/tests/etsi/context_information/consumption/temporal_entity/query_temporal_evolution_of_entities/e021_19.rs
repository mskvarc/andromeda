//! The temporal evolutions of the Vehicles are queried with the simplified temporal representation named by the
//! `format` parameter, by the `options` parameter, or by both, where `format` takes precedence over `options` (clauses
//! 5.7.3 and 6.3.12).

use crate::{
    temporal_operations::{create_temporal_entity, create_temporal_evolution, delete_temporal_entities},
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
    http::{
        headers::Accept,
        operation::{RepresentationSpelling, RepresentationToken},
    },
    model::evolution::AggregationMethod,
    query::temporal_query,
};
use serde_json::{Value, json};

/// The Vehicle `urn:ngsi-ld:Vehicle:<ns>-<suffix>` of the cases as the simplified temporal representation writes it:
/// its `speed` and `fuelLevel` each hold one object whose `values` pair every value with its `observedAt`, the
/// instants on `day` (`YYYY-MM-DD`) in UTC.
fn expected_vehicle(namespace: &Namespace, suffix: &str, day: &str) -> Value {
    json!({
        "id": namespace.urn_text("Vehicle", suffix),
        "type": "Vehicle",
        "speed": {
            "type": "Property",
            "values": [
                [120, format!("{day}T12:03:00Z")],
                [80, format!("{day}T12:05:00Z")],
                [100, format!("{day}T12:07:00Z")],
            ],
        },
        "fuelLevel": {
            "type": "Property",
            "values": [
                [67, format!("{day}T12:03:00Z")],
                [53, format!("{day}T13:05:00Z")],
                [40, format!("{day}T14:07:00Z")],
            ],
        },
    })
}

/// The setup of the tests: creates the Vehicles `<ns>-021-19-A` and `<ns>-021-19-B` in the context of their terms, and
/// answers the vocabulary they are stored under.
async fn created_vehicles(broker: &BrokerClient, namespace: &Namespace) -> Result<Vocabulary> {
    let vocabulary = create_temporal_evolution(
        broker,
        namespace,
        &VEHICLE_SPEED_FUEL_TERMS,
        &vehicle_speed_fuel_level_on(namespace, "021-19-A", "2020-08-01")?,
    )
    .await?;
    create_temporal_entity(broker, &vocabulary, &vehicle_speed_fuel_level_on(namespace, "021-19-B", "2020-09-01")?).await?;
    Ok(vocabulary)
}

/// Pins: Check that one can query the temporal evolution with format query param only
/// Clause: 5.7.3, 6.3.12
#[gtest]
#[tokio::test]
async fn e021_19_01_query_with_format_only() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = created_vehicles(&broker, &namespace).await?;

    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
        .with_responses()
        .temporal()
        .query_values(temporal_query::after("2020-01-01T12:03:00Z".parse()?))
        .entity_type("Vehicle")
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    expect_that!(
        &body,
        matches_document(json!([
            expected_vehicle(&namespace, "021-19-A", "2020-08-01"),
            expected_vehicle(&namespace, "021-19-B", "2020-09-01"),
        ]))
        .hiding("instanceId")
    );

    delete_temporal_entities(&broker, &[&namespace.urn("Vehicle", "021-19-A")?, &namespace.urn("Vehicle", "021-19-B")?]).await;
    Ok(())
}

/// Pins: Check that if both format and options are present, the value of the format parameter takes precedence
/// Clause: 5.7.3, 6.3.12
#[gtest]
#[tokio::test]
async fn e021_19_02_query_with_format_and_options() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = created_vehicles(&broker, &namespace).await?;

    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
        .with_responses()
        .temporal()
        .query_values(temporal_query::after("2020-01-01T12:03:00Z".parse()?).aggregation_methods([AggregationMethod::Sum]))
        .entity_type("Vehicle")
        .spelling(RepresentationSpelling::default().format_over_options(RepresentationToken::AggregatedValues))
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    expect_that!(
        &body,
        matches_document(json!([
            expected_vehicle(&namespace, "021-19-A", "2020-08-01"),
            expected_vehicle(&namespace, "021-19-B", "2020-09-01"),
        ]))
        .hiding("instanceId")
    );

    delete_temporal_entities(&broker, &[&namespace.urn("Vehicle", "021-19-A")?, &namespace.urn("Vehicle", "021-19-B")?]).await;
    Ok(())
}

/// Pins: Check that one can query the temporal evolution with options query param only
/// Clause: 5.7.3, 6.3.12
#[gtest]
#[tokio::test]
async fn e021_19_03_query_with_options_only() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = created_vehicles(&broker, &namespace).await?;

    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
        .with_responses()
        .temporal()
        .query_values(temporal_query::after("2020-01-01T12:03:00Z".parse()?))
        .entity_type("Vehicle")
        .spelling(RepresentationSpelling::default().in_options())
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    expect_that!(
        &body,
        matches_document(json!([
            expected_vehicle(&namespace, "021-19-A", "2020-08-01"),
            expected_vehicle(&namespace, "021-19-B", "2020-09-01"),
        ]))
        .hiding("instanceId")
    );

    delete_temporal_entities(&broker, &[&namespace.urn("Vehicle", "021-19-A")?, &namespace.urn("Vehicle", "021-19-B")?]).await;
    Ok(())
}
