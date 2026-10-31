//! An attribute is not deleted when its entity is not known to the system, or when the entity holds no instance of the
//! attribute with the dataset named (clause 5.6.5).

use crate::{
    entity_operations::delete_entities,
    vehicle_entity_payloads::VEHICLE_BRAND_PARKED_SPEED_SOURCE_TERMS,
    vehicle_fixtures::create_vehicle_brand_parked_speed_gps,
};
use andromeda_test_suite::{broker, namespace::Namespace, problem::answered};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{http::operation::InstanceSelection, model::AttributeName};

/// Pins: Check that one cannot delete an attribute from an entity with invalid/missing ids
/// Clause: 5.6.5
#[gtest]
#[tokio::test]
async fn e013_03_01_delete_an_attribute_when_the_entity_id_is_not_known_to_the_system() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    create_vehicle_brand_parked_speed_gps(&broker, &namespace, &VEHICLE_BRAND_PARKED_SPEED_SOURCE_TERMS).await?;
    let id = namespace.urn("Vehicle", 1)?;
    let unknown = namespace.urn("Vehicle", 2)?;
    let speed: AttributeName = "speed".parse()?;
    let gps = namespace.dataset_id("Property", "gpsBxyz123-speed")?;

    let answer = broker
        .with_responses()
        .entities()
        .delete_attribute(&unknown, &speed)
        .instances(InstanceSelection::Selected(Some(&gps)))
        .send()
        .await;
    verify_that!(&answer, answered(StatusCode::NOT_FOUND))?;

    delete_entities(&broker, &[&id]).await;
    Ok(())
}

/// Pins: Check that one cannot delete an attribute from an entity with invalid/missing ids
/// Clause: 5.6.5
/// Expected: notFound stays out of the file vocabulary: the test reads it without a @context, so it expands under the
/// core default vocabulary
#[gtest]
#[tokio::test]
async fn e013_03_02_delete_an_attribute_when_the_entity_does_not_contain_the_target_attribute_id() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    create_vehicle_brand_parked_speed_gps(&broker, &namespace, &VEHICLE_BRAND_PARKED_SPEED_SOURCE_TERMS).await?;
    let id = namespace.urn("Vehicle", 1)?;
    let not_found: AttributeName = "notFound".parse()?;

    let answer = broker
        .with_responses()
        .entities()
        .delete_attribute(&id, &not_found)
        .instances(InstanceSelection::Selected(None))
        .send()
        .await;
    verify_that!(&answer, answered(StatusCode::NOT_FOUND))?;

    delete_entities(&broker, &[&id]).await;
    Ok(())
}

/// Pins: Check that one cannot delete an attribute from an entity with invalid/missing ids
/// Clause: 5.6.5
#[gtest]
#[tokio::test]
async fn e013_03_03_delete_an_attribute_when_the_entity_does_not_contain_the_target_attribute_with_same_dataset_id() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    create_vehicle_brand_parked_speed_gps(&broker, &namespace, &VEHICLE_BRAND_PARKED_SPEED_SOURCE_TERMS).await?;
    let id = namespace.urn("Vehicle", 1)?;
    let speed: AttributeName = "speed".parse()?;
    let not_found = namespace.dataset_id("Property", "notFound")?;

    let answer = broker
        .with_responses()
        .entities()
        .delete_attribute(&id, &speed)
        .instances(InstanceSelection::Selected(Some(&not_found)))
        .send()
        .await;
    verify_that!(&answer, answered(StatusCode::NOT_FOUND))?;

    delete_entities(&broker, &[&id]).await;
    Ok(())
}
