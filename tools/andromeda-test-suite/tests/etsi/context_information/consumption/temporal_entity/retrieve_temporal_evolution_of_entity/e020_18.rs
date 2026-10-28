//! ETSI `020_18`: the temporal evolution of an entity is retrieved in simplified temporal representation after one of
//! its attributes is deleted, as the `deletedAt` temporal property names the instance that the deletion recorded
//! (clause 5.7.3).

use crate::suite_keywords::{create_vehicle_parking_name_fuel, delete_attribute, deleted_temporal_values};
use andromeda_test_suite::{broker, comparison::matches_document, namespace::Namespace, teardown::AnyAnswer};
use googletest::prelude::*;
use serde_json::json;

/// Pins: Check that one can retrieve the temporal evolution of an entity following the deletedAt temporal property
/// with temporal values
/// Clause: 5.7.3
/// Changed: the deleted value is matched exactly as the NGSI-LD Null, and the pair must hold it first. The
/// alternative that holds a fixed instant first is not admitted, since a deletion instant is fixed by the run that
/// recorded it.
#[gtest]
#[tokio::test]
async fn e020_18_01_with_a_property() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_vehicle_parking_name_fuel(&broker, &namespace).await?;

    let id = namespace.urn("Vehicle", 1)?;
    let client = broker.with_context(vocabulary.context())?;
    delete_attribute(&client, &id, "fuelLevel").await?;

    let body = deleted_temporal_values(&client, &id).await?;
    // The second item is the instant the deletion recorded, which the run fixes, so the expectation takes it from the
    // answer.
    let recorded = body.pointer("/fuelLevel/values/0/1");
    expect_that!(
        &body,
        matches_document(json!({
            "id": id.to_string(),
            "type": "Vehicle",
            "fuelLevel": {"type": "Property", "values": [["urn:ngsi-ld:null", recorded]]},
        }))
        .hiding("instanceId")
    );

    broker.temporal().delete(&id).send().await.any_answer();
    Ok(())
}

/// Pins: Check that one can retrieve the temporal evolution of an entity following the deletedAt temporal property
/// with temporal values
/// Clause: 5.7.3
/// Changed: the deleted object is matched exactly as the NGSI-LD Null, and the pair must hold it first. The
/// alternative that holds a fixed instant first is not admitted, since a deletion instant is fixed by the run that
/// recorded it.
#[gtest]
#[tokio::test]
async fn e020_18_02_with_a_relationship() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_vehicle_parking_name_fuel(&broker, &namespace).await?;

    let id = namespace.urn("Vehicle", 1)?;
    let client = broker.with_context(vocabulary.context())?;
    delete_attribute(&client, &id, "isParkedIn").await?;

    let body = deleted_temporal_values(&client, &id).await?;
    // The second item is the instant the deletion recorded, which the run fixes, so the expectation takes it from the
    // answer.
    let recorded = body.pointer("/isParkedIn/objects/0/1");
    expect_that!(
        &body,
        matches_document(json!({
            "id": id.to_string(),
            "type": "Vehicle",
            "isParkedIn": {"type": "Relationship", "objects": [["urn:ngsi-ld:null", recorded]]},
        }))
        .hiding("instanceId")
    );

    broker.temporal().delete(&id).send().await.any_answer();
    Ok(())
}

/// Pins: Check that one can retrieve the temporal evolution of an entity following the deletedAt temporal property
/// with temporal values
/// Clause: 5.7.3, 4.5.18
/// Changed: the deleted language map is matched exactly as the NGSI-LD Null under `@none`, and the pair must hold it
/// first. The alternative that holds a fixed instant first is not admitted, since a deletion instant is fixed by the
/// run that recorded it.
#[gtest]
#[tokio::test]
async fn e020_18_03_with_a_language_property() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_vehicle_parking_name_fuel(&broker, &namespace).await?;

    let id = namespace.urn("Vehicle", 1)?;
    let client = broker.with_context(vocabulary.context())?;
    delete_attribute(&client, &id, "name").await?;

    let body = deleted_temporal_values(&client, &id).await?;
    // The second item is the instant the deletion recorded, which the run fixes, so the expectation takes it from the
    // answer.
    let recorded = body.pointer("/name/languageMaps/0/1");
    expect_that!(
        &body,
        matches_document(json!({
            "id": id.to_string(),
            "type": "Vehicle",
            "name": {
                "type": "LanguageProperty",
                "languageMaps": [[{"languageMap": {"@none": "urn:ngsi-ld:null"}}, recorded]],
            },
        }))
        .hiding("instanceId")
    );

    broker.temporal().delete(&id).send().await.any_answer();
    Ok(())
}
