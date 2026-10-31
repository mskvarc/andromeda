//! An update whose fragment is not JSON is refused with 400 `InvalidRequest` (clause 5.6.2).

use crate::{
    entity_operations::{delete_entities, ld_json_body_client},
    unchecked_requests::with_doubled_separator,
    vehicle_entity_payloads::{VEHICLE_BRAND_PARKED_SPEED_SOURCE_TERMS, speedometer_speed_56},
    vehicle_fixtures::create_vehicle_brand_parked_speed_sources,
};
use andromeda_test_suite::{broker, namespace::Namespace, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::operation::UpdateAttributes,
    model::{EntityFragment, ErrorType},
};

/// Pins: Check that one cannot update an attribute if the entity fragment is invalid
/// Clause: 5.6.2
#[gtest]
#[tokio::test]
async fn e011_04_01_update_entity_attributes_with_invalid_entity_fragments() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_vehicle_brand_parked_speed_sources(&broker, &namespace, &VEHICLE_BRAND_PARKED_SPEED_SOURCE_TERMS).await?;
    let id = namespace.urn("Vehicle", 1)?;
    let fragment = EntityFragment::builder()
        .property("speed", speedometer_speed_56(&namespace, "speedometerA4567-speed2")?)
        .property("brandName", "BMW")
        .build()?;
    let client = ld_json_body_client(&broker, &vocabulary)?;
    let operation = UpdateAttributes::builder().id(&id).fragment(&fragment).build();
    let request = with_doubled_separator(client.unchecked(&operation)?);
    let answer = client.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest).with_title());
    delete_entities(&broker, &[&id]).await;
    Ok(())
}
