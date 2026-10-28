//! ETSI `021_03`: the last four instances of each attribute of the temporal evolutions of the Vehicles of a context are
//! queried after an instant (clause 5.7.4). A Vehicle with five instances keeps its four most recent ones, and a
//! Vehicle
//! with three keeps all of them.
//!
//! The answer is compared with the case's expectation as the suite's `Compare Dictionaries Ignoring Keys` makes it.

use crate::{
    suite_keywords::{create_temporal_entity, delete_temporal_entities},
    suite_payloads::{VEHICLE_SPEED_FUEL_TERMS, vehicle_five_instances, vehicle_speed_fuel_level_on},
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
use ngsi_ld::{http::headers::Accept, query::temporal_query};
use serde_json::json;

/// Registers the terms of the case and creates Vehicle `021-03-A`, with five instances of each attribute, and Vehicle
/// `021-03-B` observed on 2020-09-01, in the vocabulary's context.
async fn create_vehicles(broker: &BrokerClient, namespace: &Namespace) -> Result<Vocabulary> {
    let vocabulary = Vocabulary::register(broker, namespace, &VEHICLE_SPEED_FUEL_TERMS).await?;
    create_temporal_entity(broker, &vocabulary, &vehicle_five_instances(namespace, "021-03-A")?).await?;
    create_temporal_entity(broker, &vocabulary, &vehicle_speed_fuel_level_on(namespace, "021-03-B", "2020-09-01")?).await?;
    Ok(vocabulary)
}

/// Pins: Check that one can query the temporal evolution of the last N instances of entities attributes
/// Clause: 5.7.4
#[gtest]
#[tokio::test]
async fn e021_03_01_query_the_temporal_evolution_of_the_last_n_instances_of_entities_attributes() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_vehicles(&broker, &namespace).await?;

    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
        .with_responses()
        .temporal()
        .query(temporal_query::after("2020-07-01T12:05:00Z".parse()?).last_n(4))
        .entity_type("Vehicle")
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    expect_that!(
        &body,
        matches_document(json!([
            {
                "id": namespace.urn_text("Vehicle", "021-03-A"),
                "type": "Vehicle",
                "speed": [
                    {"type": "Property", "value": 105, "observedAt": "2020-08-01T12:27:00Z"},
                    {"type": "Property", "value": 120, "observedAt": "2020-08-01T12:29:00Z"},
                    {"type": "Property", "value": 125, "observedAt": "2020-08-01T12:31:00Z"},
                    {"type": "Property", "value": 130, "observedAt": "2020-08-01T12:35:00Z"},
                ],
                "fuelLevel": [
                    {"type": "Property", "value": 31, "observedAt": "2020-08-01T20:07:00Z"},
                    {"type": "Property", "value": 28, "observedAt": "2020-08-01T21:03:00Z"},
                    {"type": "Property", "value": 24, "observedAt": "2020-08-01T21:05:00Z"},
                    {"type": "Property", "value": 19, "observedAt": "2020-08-01T22:07:00Z"},
                ],
            },
            {
                "id": namespace.urn_text("Vehicle", "021-03-B"),
                "type": "Vehicle",
                "speed": [
                    {"type": "Property", "value": 120, "observedAt": "2020-09-01T12:03:00Z"},
                    {"type": "Property", "value": 80, "observedAt": "2020-09-01T12:05:00Z"},
                    {"type": "Property", "value": 100, "observedAt": "2020-09-01T12:07:00Z"},
                ],
                "fuelLevel": [
                    {"type": "Property", "value": 67, "observedAt": "2020-09-01T12:03:00Z"},
                    {"type": "Property", "value": 53, "observedAt": "2020-09-01T13:05:00Z"},
                    {"type": "Property", "value": 40, "observedAt": "2020-09-01T14:07:00Z"},
                ],
            },
        ]))
        .hiding("instanceId")
    );

    delete_temporal_entities(&broker, &[&namespace.urn("Vehicle", "021-03-A")?, &namespace.urn("Vehicle", "021-03-B")?]).await;
    Ok(())
}
