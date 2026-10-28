//! ETSI `011_02`: an update is refused with 400 `BadRequestData` when its entity id is missing or is not a URI (clause 5.6.2).

use crate::{
    suite_keywords::{create_vehicle_brand_parked_speed_gps, delete_entities, ld_json_body_client},
    suite_payloads::{VEHICLE_BRAND_PARKED_SPEED_SOURCE_TERMS, speedometer_speed_99_fragment},
};
use andromeda_test_suite::{broker, namespace::Namespace, problem::problem, wire::send_to};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{http::operation::UpdateAttributes, model::ErrorType};

/// Pins: Check that one cannot update entity attributes with invalid/missing id or invalid request body
/// Clause: 5.6.2
#[gtest]
#[tokio::test]
async fn e011_02_01_update_an_attribute_if_the_entity_id_is_not_present() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_vehicle_brand_parked_speed_gps(&broker, &namespace, &VEHICLE_BRAND_PARKED_SPEED_SOURCE_TERMS).await?;
    let id = namespace.urn("Vehicle", 1)?;
    let client = ld_json_body_client(&broker, &vocabulary)?;
    let fragment = speedometer_speed_99_fragment()?;
    let operation = UpdateAttributes::builder().id(&id).fragment(&fragment).build();
    let answer = send_to(&client, &operation, "entities//attrs/").await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    delete_entities(&broker, &[&id]).await;
    Ok(())
}

/// Pins: Check that one cannot update entity attributes with invalid/missing id or invalid request body
/// Clause: 5.6.2
#[gtest]
#[tokio::test]
async fn e011_02_02_update_an_attribute_if_the_entity_id_is_not_a_valid_uri() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_vehicle_brand_parked_speed_gps(&broker, &namespace, &VEHICLE_BRAND_PARKED_SPEED_SOURCE_TERMS).await?;
    let id = namespace.urn("Vehicle", 1)?;
    let client = ld_json_body_client(&broker, &vocabulary)?;
    let fragment = speedometer_speed_99_fragment()?;
    let operation = UpdateAttributes::builder().id(&id).fragment(&fragment).build();
    let answer = send_to(&client, &operation, "entities/thisisaninvaliduri/attrs/").await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    delete_entities(&broker, &[&id]).await;
    Ok(())
}
