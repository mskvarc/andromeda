//! ETSI `013_03`: an attribute is not deleted when its entity is not known to the system, or when the entity holds no
//! instance of the attribute with the dataset named (clause 5.6.5).

use crate::{
    suite_keywords::{answered_with, create_vehicle_brand_parked_speed_gps, delete_entities, naming_delete_all_false},
    suite_payloads::VEHICLE_BRAND_PARKED_SPEED_SOURCE_TERMS,
};
use andromeda_test_suite::{broker, namespace::Namespace};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::operation::{DeleteAttribute, InstanceSelection},
    model::AttributeName,
};

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
    let gps = namespace.urn("Property", "gpsBxyz123-speed")?;

    let operation = DeleteAttribute::builder()
        .id(&unknown)
        .name(&speed)
        .instances(InstanceSelection::Dataset(gps.as_uri()))
        .build();
    answered_with(&broker, naming_delete_all_false(&broker, &operation)?, StatusCode::NOT_FOUND).await?;

    delete_entities(&broker, &[&id]).await;
    Ok(())
}

/// Pins: Check that one cannot delete an attribute from an entity with invalid/missing ids
/// Clause: 5.6.5
/// Changed: notFound stays out of the file vocabulary: the case reads it without a @context, so it expands under the
/// core default vocabulary as in the Robot run
#[gtest]
#[tokio::test]
async fn e013_03_02_delete_an_attribute_when_the_entity_does_not_contain_the_target_attribute_id() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    create_vehicle_brand_parked_speed_gps(&broker, &namespace, &VEHICLE_BRAND_PARKED_SPEED_SOURCE_TERMS).await?;
    let id = namespace.urn("Vehicle", 1)?;
    let not_found: AttributeName = "notFound".parse()?;

    let operation = DeleteAttribute::builder().id(&id).name(&not_found).build();
    answered_with(&broker, naming_delete_all_false(&broker, &operation)?, StatusCode::NOT_FOUND).await?;

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
    let not_found = namespace.urn("Property", "notFound")?;

    let operation = DeleteAttribute::builder()
        .id(&id)
        .name(&speed)
        .instances(InstanceSelection::Dataset(not_found.as_uri()))
        .build();
    answered_with(&broker, naming_delete_all_false(&broker, &operation)?, StatusCode::NOT_FOUND).await?;

    delete_entities(&broker, &[&id]).await;
    Ok(())
}
