//! ETSI `021_07`: the temporal evolutions of the entities whose identifiers match an identifier pattern are queried
//! (clause 5.7.4).
//!
//! The answer is compared with the case's expectation as the suite's `Compare Dictionaries Ignoring Keys` makes it.

use crate::suite_keywords::{create_vehicle_pair, delete_vehicle_pair};
use andromeda_test_suite::{broker, comparison::matches_document, namespace::Namespace, problem::answered, wire::json_body};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{http::headers::Accept, query::temporal_query};
use serde_json::json;

/// Pins: Check that one can query the temporal evolution of entities matching the given id pattern
/// Clause: 5.7.4
#[gtest]
#[tokio::test]
async fn e021_07_01_query_the_temporal_evolution_of_entities_matching_the_given_id_pattern() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_vehicle_pair(&broker, &namespace, "021-07").await?;

    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
        .with_responses()
        .temporal()
        .query(temporal_query::after("2020-07-01T12:05:00Z".parse()?))
        .entity_type("Vehicle")
        .id_pattern(namespace.id_pattern("Vehicle")?)
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    expect_that!(
        &body,
        matches_document(json!([
            {
                "id": namespace.urn_text("Vehicle", "021-07-A"),
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
            },
            {
                "id": namespace.urn_text("Vehicle", "021-07-B"),
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

    delete_vehicle_pair(&broker, &namespace, "021-07").await?;
    Ok(())
}
