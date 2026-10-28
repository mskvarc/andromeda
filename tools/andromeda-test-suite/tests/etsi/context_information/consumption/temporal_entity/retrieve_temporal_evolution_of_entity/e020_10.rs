//! ETSI `020_10`: the temporal evolution of an entity is retrieved in the simplified temporal representation, each
//! attribute as the pairs of its value and observation time, named by `options=temporalValues` (clause 5.7.3).
//!
//! The answer is compared with the case's expectation as the suite's `Compare Dictionaries Ignoring Keys` makes it.

use crate::suite_keywords::create_vehicle_speed_fuel_level;
use andromeda_test_suite::{broker, comparison::matches_document, namespace::Namespace, problem::answered, teardown::AnyAnswer, wire::json_body};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::http::{headers::Accept, operation::RepresentationSpelling};
use serde_json::json;

/// Pins: Check that one can retrieve the temporal evolution of an entity with the simplified temporal representation
/// Clause: 5.7.3
#[gtest]
#[tokio::test]
async fn e020_10_01_retrieve_the_temporal_evolution_of_an_entity_with_the_simplified_temporal_representation() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_vehicle_speed_fuel_level(&broker, &namespace).await?;

    let id = namespace.urn("Vehicle", 1)?;
    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
        .with_responses()
        .temporal()
        .retrieve_values(&id)
        .spelling(RepresentationSpelling::default().in_options())
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    let expected = json!({
        "id": id.to_string(),
        "type": "Vehicle",
        "speed": {
            "type": "Property",
            "values": [
                [120, "2020-08-01T12:03:00Z"],
                [80, "2020-08-01T12:05:00Z"],
                [100, "2020-08-01T12:07:00Z"],
            ],
        },
        "fuelLevel": {
            "type": "Property",
            "values": [
                [67, "2020-08-01T12:03:00Z"],
                [53, "2020-08-01T13:05:00Z"],
                [40, "2020-08-01T14:07:00Z"],
            ],
        },
    });
    expect_that!(&body, matches_document(expected).hiding("instanceId"));

    broker.temporal().delete(&id).send().await.any_answer();
    Ok(())
}
