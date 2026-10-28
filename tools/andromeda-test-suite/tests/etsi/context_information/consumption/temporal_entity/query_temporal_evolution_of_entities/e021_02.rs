//! ETSI `021_02`: the temporal evolutions of the Vehicles of a context are queried for the `speed` attribute alone
//! (clause 5.7.4), so the answer holds no `fuelLevel`.
//!
//! The answer is compared with the case's expectation as the suite's `Compare Dictionaries Ignoring Keys` makes it.

use crate::suite_keywords::{create_vehicle_pair, delete_vehicle_pair};
use andromeda_test_suite::{broker, comparison::matches_document, namespace::Namespace, problem::answered, wire::json_body};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{http::headers::Accept, query::temporal_query};
use serde_json::json;

/// Pins: Check that one can query the temporal evolution of certain attributes of entities
/// Clause: 5.7.4
#[gtest]
#[tokio::test]
async fn e021_02_01_query_the_temporal_evolution_of_certain_attributes_of_entities() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_vehicle_pair(&broker, &namespace, "021-02").await?;

    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
        .with_responses()
        .temporal()
        .query(temporal_query::after("2020-07-01T12:05:00Z".parse()?))
        .entity_type("Vehicle")
        .attrs(vec!["speed".parse()?])
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    expect_that!(
        &body,
        matches_document(json!([
            {
                "id": namespace.urn_text("Vehicle", "021-02-A"),
                "type": "Vehicle",
                "speed": [
                    {"type": "Property", "value": 120, "observedAt": "2020-08-01T12:03:00Z"},
                    {"type": "Property", "value": 80, "observedAt": "2020-08-01T12:05:00Z"},
                    {"type": "Property", "value": 100, "observedAt": "2020-08-01T12:07:00Z"},
                ],
            },
            {
                "id": namespace.urn_text("Vehicle", "021-02-B"),
                "type": "Vehicle",
                "speed": [
                    {"type": "Property", "value": 120, "observedAt": "2020-09-01T12:03:00Z"},
                    {"type": "Property", "value": 80, "observedAt": "2020-09-01T12:05:00Z"},
                    {"type": "Property", "value": 100, "observedAt": "2020-09-01T12:07:00Z"},
                ],
            },
        ]))
        .hiding("instanceId")
    );

    delete_vehicle_pair(&broker, &namespace, "021-02").await?;
    Ok(())
}
