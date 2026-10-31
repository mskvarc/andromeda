//! A temporal representation of an entity is created with simple temporal properties, with and without an `@context` in
//! the body (clause 5.6.11).

use crate::{
    entity_operations::ld_json_body_client,
    temporal_operations::retrieve_temporal_json,
    vehicle_evolution_payloads::{VEHICLE_SPEED_FUEL_TERMS, vehicle_speed_fuel_level_on, vehicle_speed_fuel_level_with_dataset},
};
use andromeda_test_suite::{
    broker,
    comparison::matches_document,
    namespace::Namespace,
    problem::answered,
    teardown::AnyAnswer,
    vocabulary::Vocabulary,
    wire::{api_uri, content_type, send_to},
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{headers::Accept, operation::CreateOrUpdateTemporalEvolution},
    model::APPLICATION_JSON,
};
use serde_json::json;

/// Pins: Check that one can create a temporal representation of an entity
/// Clause: 5.6.11
#[gtest]
#[tokio::test]
async fn e007_01_01_create_a_temporal_representation_of_an_entity() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &VEHICLE_SPEED_FUEL_TERMS).await?;
    let evolution = vehicle_speed_fuel_level_with_dataset(&namespace)?;
    let client = ld_json_body_client(&broker, &vocabulary)?;
    // The target carries a trailing `/`, which no typed operation writes, so the typed request is sent to it.
    let created = send_to(
        &client,
        &CreateOrUpdateTemporalEvolution::builder().entity(&evolution).build(),
        "temporal/entities/",
    )
    .await;
    verify_that!(&created, answered(StatusCode::CREATED))?;
    expect_that!(created?.body().len(), eq(0));

    let id = namespace.urn("Vehicle", 1)?;
    let client = broker.with_context(vocabulary.context())?.with_accept(Accept::any());
    // The answer is compared whole, absent members included, which no typed matcher states, so it is read as JSON.
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
        }))
        .hiding("instanceId")
        .hiding_everywhere("@context")
    );

    broker.temporal().delete(&id).send().await.any_answer();
    Ok(())
}

/// Pins: Check that one can create a temporal representation of an entity
/// Clause: 5.6.11
/// Vocabulary: none (the test references no @context)
#[gtest]
#[tokio::test]
async fn e007_01_02_create_a_temporal_entity_with_no_context() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let evolution = vehicle_speed_fuel_level_on(&namespace, 1, "2020-09-01")?;
    let operation = CreateOrUpdateTemporalEvolution::builder().entity(&evolution).build();
    // The target carries a trailing `/`, which no typed operation writes, so the typed request is sent to it.
    let request = content_type(broker.unchecked(&operation)?.uri(api_uri(&broker, "temporal/entities/")?), &APPLICATION_JSON)?;
    let created = broker.send_unchecked(request).await;
    verify_that!(&created, answered(StatusCode::CREATED))?;
    expect_that!(created?.body().len(), eq(0));

    let id = namespace.urn("Vehicle", 1)?;
    // The answer is compared whole, absent members included, which no typed matcher states, so it is read as JSON.
    let body = retrieve_temporal_json(&broker.with_accept(Accept::any()), &id).await?;
    expect_that!(
        &body,
        matches_document(json!({
            "id": id.to_string(),
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
        }))
        .hiding("instanceId")
    );

    broker.temporal().delete(&id).send().await.any_answer();
    Ok(())
}
