//! An append whose path names no entity id, or an id that is not a URI, is refused with 400 `BadRequestData` (clause
//! 5.6.3).

use crate::{
    entity_operations::{delete_entities, ld_json_body_client},
    vehicle_entity_payloads::{VEHICLE_BRAND_PARKED_SPEED_SOURCE_ADDED_TERMS, speed_and_added_attribute},
    vehicle_fixtures::create_vehicle_brand_parked_speed_sources,
};
use andromeda_test_suite::{broker, namespace::Namespace, problem::problem, wire::send_to};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{http::operation::AppendAttributes, model::ErrorType};

/// Pins: Check that one cannot append entity attributes with invalid/missing id or invalid request body
/// Clause: 5.6.3
#[gtest]
#[tokio::test]
async fn e010_02_01_append_entity_attributes_if_the_entity_id_is_not_present() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_vehicle_brand_parked_speed_sources(&broker, &namespace, &VEHICLE_BRAND_PARKED_SPEED_SOURCE_ADDED_TERMS).await?;
    let id = namespace.urn("Vehicle", 1)?;
    let client = ld_json_body_client(&broker, &vocabulary)?;
    let fragment = speed_and_added_attribute(&namespace, "speedometer")?;
    let operation = AppendAttributes::builder().id(&id).fragment(&fragment).build();
    let answer = send_to(&client, &operation, "entities//attrs/").await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    delete_entities(&broker, &[&id]).await;
    Ok(())
}

/// Pins: Check that one cannot append entity attributes with invalid/missing id or invalid request body
/// Clause: 5.6.3
#[gtest]
#[tokio::test]
async fn e010_02_02_append_entity_attributes_if_the_entity_id_is_not_a_valid_uri() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_vehicle_brand_parked_speed_sources(&broker, &namespace, &VEHICLE_BRAND_PARKED_SPEED_SOURCE_ADDED_TERMS).await?;
    let id = namespace.urn("Vehicle", 1)?;
    let client = ld_json_body_client(&broker, &vocabulary)?;
    let fragment = speed_and_added_attribute(&namespace, "speedometer")?;
    let operation = AppendAttributes::builder().id(&id).fragment(&fragment).build();
    let answer = send_to(&client, &operation, "entities/thisisaninvaliduri/attrs/").await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    delete_entities(&broker, &[&id]).await;
    Ok(())
}
