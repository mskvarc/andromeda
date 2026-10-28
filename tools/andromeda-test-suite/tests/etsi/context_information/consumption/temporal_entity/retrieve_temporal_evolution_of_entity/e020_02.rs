//! ETSI `020_02`: the temporal evolution of an entity is retrieved with the vocabulary named in the `Link` header, so
//! its terms come back compacted (clause 5.7.3).
//!
//! The answer is compared with the case's expectation as the suite's `Compare Dictionaries Ignoring Keys` makes it.

use crate::suite_keywords::create_vehicle_speed_fuel_level;
use andromeda_test_suite::{broker, comparison::matches_document, namespace::Namespace, problem::answered, teardown::AnyAnswer, wire::json_body};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::http::headers::Accept;
use serde_json::json;

/// Pins: Check that one can retrieve the temporal evolution of an entity using a context
/// Clause: 5.7.3
#[gtest]
#[tokio::test]
async fn e020_02_01_retrieve_the_temporal_evolution_of_an_entity_using_a_context() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_vehicle_speed_fuel_level(&broker, &namespace).await?;

    let id = namespace.urn("Vehicle", 1)?;
    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
        .with_responses()
        .temporal()
        .retrieve(&id)
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    let expected = json!({
        "id": id.to_string(),
        "type": "Vehicle",
        "speed": [
            {"type": "Property", "value": 120, "observedAt": "2020-08-01T12:03:00Z"},
            {"type": "Property", "value": 80, "observedAt": "2020-08-01T12:05:00Z"},
            {"type": "Property", "value": 100, "observedAt": "2020-08-01T12:07:00Z"},
        ],
        "fuelLevel": [
            {"type": "Property", "value": 67, "observedAt": "2020-08-01T12:03:00Z"},
            {"type": "Property", "value": 53, "observedAt": "2020-08-01T13:05:00Z"},
            {"type": "Property", "value": 40, "observedAt": "2020-08-01T14:07:00Z"},
        ],
    });
    expect_that!(&body, matches_document(expected).hiding("instanceId"));

    broker.temporal().delete(&id).send().await.any_answer();
    Ok(())
}
