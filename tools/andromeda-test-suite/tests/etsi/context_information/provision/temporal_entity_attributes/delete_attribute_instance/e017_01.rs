//! ETSI `017_01`: an attribute instance is deleted from a temporal representation of an entity (clause 5.6.15).

use crate::suite_keywords::{create_vehicle_speed_fuel_level_with_dataset, first_speed_instance_id, retrieve_temporal_json};
use andromeda_test_suite::{broker, comparison::matches_document, namespace::Namespace, problem::answered, teardown::AnyAnswer};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::model::{AttributeName, NgsiLdMediaType};
use serde_json::json;

/// Pins: Check that one can delete an attribute instance in temporal representation of an entity
/// Clause: 5.6.15
#[gtest]
#[tokio::test]
async fn e017_01_01_delete_an_attribute_instance_in_temporal_representation_of_an_entity() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_vehicle_speed_fuel_level_with_dataset(&broker, &namespace).await?;
    let id = namespace.urn("Vehicle", 1)?;
    let speed: AttributeName = "speed".parse()?;
    let client = broker.with_context(vocabulary.context())?;

    let before = retrieve_temporal_json(&client.with_accept(NgsiLdMediaType::JsonLd), &id).await?;
    let instance_id = first_speed_instance_id(&before)?;
    let deleted = client.with_responses().temporal().delete_instance(&id, &speed, &instance_id).send().await;
    verify_that!(&deleted, answered(StatusCode::NO_CONTENT))?;
    expect_that!(deleted?.body.len(), eq(0));

    let body = retrieve_temporal_json(&client.with_accept(NgsiLdMediaType::JsonLd), &id).await?;
    expect_that!(
        &body,
        matches_document(json!({
            "id": id.to_string(),
            "type": "Vehicle",
            "speed": [
                {"type": "Property", "value": 80, "observedAt": "2020-09-01T12:05:00Z"},
            ],
            "fuelLevel": [
                {"type": "Property", "value": 67, "observedAt": "2020-09-01T12:03:00Z"},
                {"type": "Property", "value": 53, "observedAt": "2020-09-01T13:05:00Z"},
                {
                    "type": "Property",
                    "value": 40,
                    "observedAt": "2020-09-01T14:07:00Z",
                    "datasetId": namespace.urn_text("Vehicle", "12345-fuel"),
                },
            ],
        }))
        .hiding("instanceId")
        .hiding_everywhere("@context")
    );

    broker.temporal().delete(&id).send().await.any_answer();
    Ok(())
}
