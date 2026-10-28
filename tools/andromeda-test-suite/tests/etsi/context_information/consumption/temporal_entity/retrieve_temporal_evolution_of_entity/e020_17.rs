//! ETSI `020_17`: the temporal evolution of an entity is retrieved after one of its attributes is deleted, as the
//! `deletedAt` temporal property names the instances the deletion removed (clause 5.7.3).
//!
//! The answer is compared with the case's expectation as the suite's `Compare Dictionaries Ignoring Keys` makes it.

use crate::suite_keywords::{create_vehicle_parking_name_fuel, delete_attribute};
use andromeda_test_suite::{broker, comparison::matches_document, namespace::Namespace, problem::answered, teardown::AnyAnswer, wire::json_body};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{http::headers::Accept, query::temporal_query::TemporalProperty};
use serde_json::json;

/// Pins: Check that one can retrieve the temporal evolution of an entity following the deletedAt temporal property
/// Clause: 5.7.3
#[gtest]
#[tokio::test]
async fn e020_17_01_with_a_property() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_vehicle_parking_name_fuel(&broker, &namespace).await?;

    let id = namespace.urn("Vehicle", 1)?;
    let client = broker.with_context(vocabulary.context())?;
    delete_attribute(&client, &id, "fuelLevel").await?;

    let received = client
        .with_accept(Accept::any())
        .with_responses()
        .temporal()
        .retrieve(&id)
        .time_property(TemporalProperty::DeletedAt)
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    let expected = json!({
        "id": id.to_string(),
        "type": "Vehicle",
        "fuelLevel": [{"type": "Property", "value": "urn:ngsi-ld:null"}],
    });
    expect_that!(&body, matches_document(expected).hiding_everywhere("deletedAt").hiding("instanceId"));
    expect_that!(body.pointer("/fuelLevel/0/deletedAt").is_some(), is_true());

    broker.temporal().delete(&id).send().await.any_answer();
    Ok(())
}

/// Pins: Check that one can retrieve the temporal evolution of an entity following the deletedAt temporal property
/// Clause: 5.7.3
#[gtest]
#[tokio::test]
async fn e020_17_02_with_a_relationship() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_vehicle_parking_name_fuel(&broker, &namespace).await?;

    let id = namespace.urn("Vehicle", 1)?;
    let client = broker.with_context(vocabulary.context())?;
    delete_attribute(&client, &id, "isParkedIn").await?;

    let received = client
        .with_accept(Accept::any())
        .with_responses()
        .temporal()
        .retrieve(&id)
        .time_property(TemporalProperty::DeletedAt)
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    let expected = json!({
        "id": id.to_string(),
        "type": "Vehicle",
        "isParkedIn": [{"type": "Relationship", "object": "urn:ngsi-ld:null"}],
    });
    expect_that!(&body, matches_document(expected).hiding_everywhere("deletedAt").hiding("instanceId"));
    expect_that!(body.pointer("/isParkedIn/0/deletedAt").is_some(), is_true());

    broker.temporal().delete(&id).send().await.any_answer();
    Ok(())
}

/// Pins: Check that one can retrieve the temporal evolution of an entity following the deletedAt temporal property
/// Clause: 5.7.3, 4.5.18
#[gtest]
#[tokio::test]
async fn e020_17_03_with_a_language_property() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_vehicle_parking_name_fuel(&broker, &namespace).await?;

    let id = namespace.urn("Vehicle", 1)?;
    let client = broker.with_context(vocabulary.context())?;
    delete_attribute(&client, &id, "name").await?;

    let received = client
        .with_accept(Accept::any())
        .with_responses()
        .temporal()
        .retrieve(&id)
        .time_property(TemporalProperty::DeletedAt)
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    let expected = json!({
        "id": id.to_string(),
        "type": "Vehicle",
        "name": [{"type": "LanguageProperty", "languageMap": {"@none": "urn:ngsi-ld:null"}}],
    });
    expect_that!(&body, matches_document(expected).hiding_everywhere("deletedAt").hiding("instanceId"));
    expect_that!(body.pointer("/name/0/deletedAt").is_some(), is_true());

    broker.temporal().delete(&id).send().await.any_answer();
    Ok(())
}
