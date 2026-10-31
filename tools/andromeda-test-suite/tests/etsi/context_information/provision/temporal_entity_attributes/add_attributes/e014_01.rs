//! Instances of an attribute added to a temporal representation of an entity are appended to its evolution (clause
//! 5.6.12).

use crate::{
    entity_operations::ld_json_body_client,
    temporal_operations::{create_temporal_evolution, retrieve_temporal_json},
    vehicle_evolution_payloads::{VEHICLE_SPEED_FUEL_ADDED_TERMS, added_attribute_instances, vehicle_speed_fuel_level_with_dataset},
};
use andromeda_test_suite::{broker, comparison::matches_document, namespace::Namespace, problem::answered, teardown::AnyAnswer};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::model::NgsiLdMediaType;
use serde_json::json;

/// Pins: Check that one can add a simple temporal attribute to a temporal representation of an entity
/// Clause: 5.6.12
#[gtest]
#[tokio::test]
async fn e014_01_01_add_an_attribute_to_a_temporal_entity_with_simple_temporal_properties() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_temporal_evolution(
        &broker,
        &namespace,
        &VEHICLE_SPEED_FUEL_ADDED_TERMS,
        &vehicle_speed_fuel_level_with_dataset(&namespace)?,
    )
    .await?;

    let id = namespace.urn("Vehicle", 1)?;
    let client = ld_json_body_client(&broker, &vocabulary)?;
    let added = client
        .with_responses()
        .temporal()
        .add_attributes(&id, &added_attribute_instances()?)
        .send()
        .await;
    verify_that!(&added, answered(StatusCode::NO_CONTENT))?;
    expect_that!(added?.body.len(), eq(0));

    let client = broker.with_context(vocabulary.context())?.with_accept(NgsiLdMediaType::JsonLd);
    let body = retrieve_temporal_json(&client, &id).await?;
    expect_that!(
        &body,
        matches_document(json!({
            "id": id.to_string(),
            "type": "Vehicle",
            "speed": [
                {"type": "Property", "value": 120, "observedAt": "2020-09-01T12:03:00Z"},
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
            "added_attribute": [
                {"type": "Property", "value": 120, "observedAt": "2020-09-01T12:03:00Z"},
                {"type": "Property", "value": 80, "observedAt": "2020-09-01T12:05:00Z"},
                {"type": "Property", "value": 100, "observedAt": "2020-09-01T12:07:00Z"},
            ],
        }))
        .hiding("instanceId")
        .hiding_everywhere("@context")
    );

    broker.temporal().delete(&id).send().await.any_answer();
    Ok(())
}
