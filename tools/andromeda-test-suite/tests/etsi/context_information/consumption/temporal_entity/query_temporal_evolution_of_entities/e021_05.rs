//! ETSI `021_05`: the temporal evolutions of the entities of one type are queried, and only the Bus of the setup is
//! answered for the Bus type (clause 5.7.4).
//!
//! The answer is compared with the case's expectation as the suite's `Compare Dictionaries Ignoring Keys` makes it.

use crate::{
    suite_keywords::{create_temporal_entity, delete_temporal_entities},
    suite_payloads::{bus_speed_fuel_level, vehicle_speed_fuel_level_on},
};
use andromeda_test_suite::{broker, comparison::matches_document, namespace::Namespace, problem::answered, vocabulary::Vocabulary, wire::json_body};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{http::headers::Accept, query::temporal_query};
use serde_json::json;

/// Pins: Check that one can query the temporal evolution of entities matching the given type(s)
/// Clause: 5.7.4
#[gtest]
#[tokio::test]
async fn e021_05_01_query_the_temporal_evolution_of_entities_matching_the_given_type_s() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Vehicle", "speed", "fuelLevel", "Bus"]).await?;
    create_temporal_entity(&broker, &vocabulary, &vehicle_speed_fuel_level_on(&namespace, "021-05-A", "2020-08-01")?).await?;
    create_temporal_entity(&broker, &vocabulary, &bus_speed_fuel_level(&namespace, "021-05-A")?).await?;

    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
        .with_responses()
        .temporal()
        .query(temporal_query::after("2020-07-01T12:05:00Z".parse()?))
        .entity_type("Bus")
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    expect_that!(
        &body,
        matches_document(json!([
            {
                "id": namespace.urn_text("Bus", "021-05-A"),
                "type": "Bus",
                "speed": [
                    {"type": "Property", "value": 45, "observedAt": "2020-08-01T12:03:00Z"},
                    {"type": "Property", "value": 25, "observedAt": "2020-08-01T12:05:00Z"},
                    {"type": "Property", "value": 67, "observedAt": "2020-08-01T12:07:00Z"},
                ],
                "fuelLevel": [
                    {"type": "Property", "value": 210, "observedAt": "2020-08-01T12:03:00Z"},
                    {"type": "Property", "value": 145, "observedAt": "2020-08-01T13:05:00Z"},
                    {"type": "Property", "value": 124, "observedAt": "2020-08-01T14:07:00Z"},
                ],
            },
        ]))
        .hiding("instanceId")
    );

    delete_temporal_entities(&broker, &[&namespace.urn("Vehicle", "021-05-A")?, &namespace.urn("Bus", "021-05-A")?]).await;
    Ok(())
}
