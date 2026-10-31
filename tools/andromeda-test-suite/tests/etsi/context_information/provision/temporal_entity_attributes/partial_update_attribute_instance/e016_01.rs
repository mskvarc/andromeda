//! An attribute instance of a temporal representation of an entity is replaced by a partial update (clause 5.6.14).

use crate::{
    temporal_operations::{first_speed_instance_id, retrieve_temporal_json},
    vehicle_evolution_payloads::replacement_speed_instance,
    vehicle_fixtures::create_vehicle_speed_fuel_level_with_dataset,
};
use andromeda_test_suite::{broker, comparison::matches_document, namespace::Namespace, problem::answered, teardown::AnyAnswer};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::model::{AttributeName, NgsiLdMediaType};
use serde_json::json;

/// Pins: Check that one can partially update an attribute instance of a temporal representation of an entity
/// Clause: 5.6.14
/// Expected: the request body is the `EntityTemporal` Fragment clause 5.6.14 defines, not a bare instance (clause
/// 5.6.14.3)
#[gtest]
#[tokio::test]
async fn e016_01_01_modify_attribute_instance_in_temporal_representation_of_an_entity() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_vehicle_speed_fuel_level_with_dataset(&broker, &namespace).await?;
    let id = namespace.urn("Vehicle", 1)?;
    let speed: AttributeName = "speed".parse()?;
    let client = broker.with_context(vocabulary.context())?;

    let before = retrieve_temporal_json(&client.with_accept(NgsiLdMediaType::JsonLd), &id).await?;
    let instance_id = first_speed_instance_id(&before)?;
    let modified = client
        .with_responses()
        .temporal()
        .modify_instance(&id, &speed, &instance_id, &replacement_speed_instance()?)
        .send()
        .await;
    verify_that!(&modified, answered(StatusCode::NO_CONTENT))?;
    expect_that!(modified?.body.len(), eq(0));

    let body = retrieve_temporal_json(&client.with_accept(NgsiLdMediaType::JsonLd), &id).await?;
    expect_that!(body.pointer("/speed/0/instanceId"), some(eq(&json!(instance_id.to_string()))));
    expect_that!(
        &body,
        matches_document(json!({
            "id": id.to_string(),
            "type": "Vehicle",
            "speed": [
                {"type": "Property", "value": 129, "observedAt": "2020-09-01T12:03:00Z"},
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
        .hiding_everywhere("modifiedAt")
    );

    broker.temporal().delete(&id).send().await.any_answer();
    Ok(())
}
