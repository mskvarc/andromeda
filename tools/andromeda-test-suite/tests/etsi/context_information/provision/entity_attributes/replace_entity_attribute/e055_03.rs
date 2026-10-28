//! ETSI `055_03`: an attribute is not replaced when its entity id, attribute name or entity is invalid or not found
//! (clauses 5.6.19 and 6.7.3.3).

use crate::{
    suite_keywords::{answered_with, create_vehicle_parked_speed_sources, delete_entities},
    suite_payloads::{VEHICLE_BRAND_PARKED_SPEED_SOURCE_TERMS, VEHICLE_PARKED_SPEED_SOURCE_TERMS, is_parked_downtown2},
};
use andromeda_test_suite::{broker, namespace::Namespace, problem::answered, wire::api_uri};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, operation::ReplaceAttribute},
    model::{Attribute, AttributeName},
};

/// Pins: Check that the correct error type is returned when replacing an attribute with faulty data
/// Clause: 5.6.19, 6.7.3.3
#[gtest]
#[tokio::test]
async fn e055_03_01_replace_entity_attribute_giving_an_invalid_entity_id() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_vehicle_parked_speed_sources(&broker, &namespace, &VEHICLE_PARKED_SPEED_SOURCE_TERMS).await?;
    let id = namespace.urn("Vehicle", 1)?;
    let is_parked: AttributeName = "isParked".parse()?;
    let client = broker.with_context(vocabulary.context())?;
    let replacement = Attribute::from(is_parked_downtown2(&namespace)?);

    let operation = ReplaceAttribute::builder().id(&id).name(&is_parked).attribute(&replacement).build();
    let request = client.unchecked(&operation)?.uri(api_uri(&client, "entities/invalidUri/attrs/isParked")?);
    answered_with(&client, request, StatusCode::BAD_REQUEST).await?;

    delete_entities(&broker, &[&id]).await;
    Ok(())
}

/// Pins: Check that the correct error type is returned when replacing an attribute with faulty data
/// Clause: 5.6.19, 6.7.3.3
#[gtest]
#[tokio::test]
async fn e055_03_02_replace_entity_attribute_giving_a_nonexistent_entity_id() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_vehicle_parked_speed_sources(&broker, &namespace, &VEHICLE_PARKED_SPEED_SOURCE_TERMS).await?;
    let id = namespace.urn("Vehicle", 1)?;
    let missing = namespace.urn("Vehicle", "Nonexistent")?;
    let is_parked: AttributeName = "isParked".parse()?;
    let client = broker.with_context(vocabulary.context())?;
    let replacement = Attribute::from(is_parked_downtown2(&namespace)?);

    let answer = client
        .with_responses()
        .entities()
        .replace_attribute(&missing, &is_parked, &replacement)
        .send()
        .await;
    expect_that!(&answer, answered(StatusCode::NOT_FOUND));

    delete_entities(&broker, &[&id]).await;
    Ok(())
}

/// Pins: Check that the correct error type is returned when replacing an attribute with faulty data
/// Clause: 5.6.19, 6.7.3.3
#[gtest]
#[tokio::test]
async fn e055_03_03_replace_entity_attribute_giving_an_invalid_attribute_name() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_vehicle_parked_speed_sources(&broker, &namespace, &VEHICLE_PARKED_SPEED_SOURCE_TERMS).await?;
    let id = namespace.urn("Vehicle", 1)?;
    let is_parked: AttributeName = "isParked".parse()?;
    let client = broker.with_context(vocabulary.context())?;
    let replacement = Attribute::from(is_parked_downtown2(&namespace)?);

    let operation = ReplaceAttribute::builder().id(&id).name(&is_parked).attribute(&replacement).build();
    let request = client.unchecked(&operation)?.uri(api_uri(&client, &format!("entities/{id}/attrs/@invalid"))?);
    answered_with(&client, request, StatusCode::BAD_REQUEST).await?;

    delete_entities(&broker, &[&id]).await;
    Ok(())
}

/// Pins: Check that the correct error type is returned when replacing an attribute with faulty data
/// Clause: 5.6.19, 6.7.3.3
#[gtest]
#[tokio::test]
async fn e055_03_04_replace_entity_attribute_giving_a_nonexistent_attribute_name() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_vehicle_parked_speed_sources(&broker, &namespace, &VEHICLE_BRAND_PARKED_SPEED_SOURCE_TERMS).await?;
    let id = namespace.urn("Vehicle", 1)?;
    let brand: AttributeName = "brandName".parse()?;
    let client = broker.with_context(vocabulary.context())?;
    let replacement = Attribute::from(is_parked_downtown2(&namespace)?);

    let answer = client.with_responses().entities().replace_attribute(&id, &brand, &replacement).send().await;
    expect_that!(&answer, answered(StatusCode::NOT_FOUND));

    delete_entities(&broker, &[&id]).await;
    Ok(())
}

/// Pins: Check that the correct error type is returned when replacing an attribute with faulty data
/// Clause: 5.6.19, 6.7.3.3
#[gtest]
#[tokio::test]
async fn e055_03_05_replace_entity_attribute_giving_a_scope_attribute() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_vehicle_parked_speed_sources(&broker, &namespace, &VEHICLE_PARKED_SPEED_SOURCE_TERMS).await?;
    let id = namespace.urn("Vehicle", 1)?;
    let is_parked: AttributeName = "isParked".parse()?;
    let client = broker.with_context(vocabulary.context())?;
    let replacement = Attribute::from(is_parked_downtown2(&namespace)?);

    let operation = ReplaceAttribute::builder().id(&id).name(&is_parked).attribute(&replacement).build();
    let scope_body = UncheckedDocument::empty().set_text("scope", "/Company123/UnitA");
    let request = client
        .unchecked(&operation)?
        .uri(api_uri(&client, &format!("entities/{id}/attrs/scope"))?)
        .document(&scope_body);
    answered_with(&client, request, StatusCode::BAD_REQUEST).await?;

    delete_entities(&broker, &[&id]).await;
    Ok(())
}
