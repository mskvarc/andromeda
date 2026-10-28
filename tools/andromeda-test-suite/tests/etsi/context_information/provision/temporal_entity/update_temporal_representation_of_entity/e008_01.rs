//! ETSI `008_01`: an upsert of instances into an existing temporal representation of an entity appends them, and the
//! entity is retrieved with every instance it holds (clause 5.6.11).

use crate::{
    suite_keywords::{create_temporal_evolution, ld_json_body_client, retrieve_temporal_json},
    suite_payloads::{VEHICLE_SPEED_FUEL_TERMS, vehicle_speed_fuel_level_with_dataset},
};
use andromeda_test_suite::{broker, comparison::matches_document, namespace::Namespace, problem::answered, teardown::AnyAnswer, wire::send_to};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::operation::CreateOrUpdateTemporalEvolution,
    model::{Input, NgsiLdMediaType, Property, evolution::TemporalEntity},
};
use serde_json::json;

/// The Vehicle `urn:ngsi-ld:Vehicle:<ns>-1` that the update sends: a `speed` of 121, 80 and 100 observed at 12:03, 12:05
/// and 12:07, and a `fuelLevel` of 67, 53 and 40 observed at 12:03, 13:05 and 14:07, all on 2020-09-01 in UTC.
fn updated_vehicle(namespace: &Namespace) -> Result<TemporalEntity<Input>> {
    Ok(TemporalEntity::builder()
        .id(namespace.urn("Vehicle", 1)?)
        .entity_type("Vehicle")
        .property("speed", Property::builder(121).observed_at("2020-09-01T12:03:00Z".parse()?))
        .property("speed", Property::builder(80).observed_at("2020-09-01T12:05:00Z".parse()?))
        .property("speed", Property::builder(100).observed_at("2020-09-01T12:07:00Z".parse()?))
        .property("fuelLevel", Property::builder(67).observed_at("2020-09-01T12:03:00Z".parse()?))
        .property("fuelLevel", Property::builder(53).observed_at("2020-09-01T13:05:00Z".parse()?))
        .property("fuelLevel", Property::builder(40).observed_at("2020-09-01T14:07:00Z".parse()?))
        .build()?)
}

/// Pins: Check that one can update a temporal representation of an entity with simple temporal properties
/// Clause: 5.6.11
/// Corrected: `fix(008_01_01)`: temporal upsert appends instances, not replace (5.6.11)
/// Changed: the expectation lists an attribute instance more than once (clause 5.6.11.4 adds every instance of an upsert,
///     an equal one included); DeepDiff (`ignore_order`) compares the distinct instances only and ignores how often each
///     occurs, while the port requires each instance as often as the expectation lists it and counts a repeated one by
///     its exact expected content, observedAt in the expected text
#[gtest]
#[tokio::test]
async fn e008_01_01_update_a_temporal_representation_of_an_entity_with_simple_temporal_properties() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_temporal_evolution(
        &broker,
        &namespace,
        &VEHICLE_SPEED_FUEL_TERMS,
        &vehicle_speed_fuel_level_with_dataset(&namespace)?,
    )
    .await?;

    let client = ld_json_body_client(&broker, &vocabulary)?;
    let evolution = updated_vehicle(&namespace)?;
    let updated = send_to(
        &client,
        &CreateOrUpdateTemporalEvolution::builder().entity(&evolution).build(),
        "temporal/entities/",
    )
    .await;
    verify_that!(&updated, answered(StatusCode::NO_CONTENT))?;
    expect_that!(updated?.body().len(), eq(0));

    let id = namespace.urn("Vehicle", 1)?;
    let client = broker.with_context(vocabulary.context())?.with_accept(NgsiLdMediaType::JsonLd);
    let body = retrieve_temporal_json(&client, &id).await?;
    expect_that!(
        &body,
        matches_document(json!({
            "id": id.to_string(),
            "type": "Vehicle",
            "speed": [
                {"type": "Property", "value": 120, "observedAt": "2020-09-01T12:03:00Z"},
                {"type": "Property", "value": 121, "observedAt": "2020-09-01T12:03:00Z"},
                {"type": "Property", "value": 80, "observedAt": "2020-09-01T12:05:00Z"},
                {"type": "Property", "value": 80, "observedAt": "2020-09-01T12:05:00Z"},
                {"type": "Property", "value": 100, "observedAt": "2020-09-01T12:07:00Z"},
            ],
            "fuelLevel": [
                {"type": "Property", "value": 67, "observedAt": "2020-09-01T12:03:00Z"},
                {"type": "Property", "value": 67, "observedAt": "2020-09-01T12:03:00Z"},
                {"type": "Property", "value": 53, "observedAt": "2020-09-01T13:05:00Z"},
                {"type": "Property", "value": 53, "observedAt": "2020-09-01T13:05:00Z"},
                {
                    "type": "Property",
                    "value": 40,
                    "observedAt": "2020-09-01T14:07:00Z",
                    "datasetId": namespace.urn_text("Vehicle", "12345-fuel"),
                },
                {"type": "Property", "value": 40, "observedAt": "2020-09-01T14:07:00Z"},
            ],
        }))
        .hiding("instanceId")
        .hiding_everywhere("@context")
    );

    broker.temporal().delete(&id).send().await.any_answer();
    Ok(())
}
