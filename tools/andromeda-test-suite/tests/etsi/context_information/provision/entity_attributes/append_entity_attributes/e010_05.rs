//! ETSI `010_05`: an append whose body is not JSON, because a member separator is doubled, is refused with 400
//! `InvalidRequest` (clause 5.6.3).

use crate::{
    suite_keywords::{create_vehicle_brand_parked_speed_sources, delete_entities, ld_json_body_client, with_doubled_separator},
    suite_payloads::{VEHICLE_BRAND_PARKED_SPEED_SOURCE_TERMS, speedometer_speed_56},
};
use andromeda_test_suite::{broker, namespace::Namespace, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::operation::AppendAttributes,
    model::{EntityFragment, ErrorType},
};

/// Pins: Check that one cannot append entity attributes with invalid entity fragments
/// Clause: 5.6.3
#[gtest]
#[tokio::test]
async fn e010_05_01_append_entity_attributes_with_invalid_entity_fragments() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_vehicle_brand_parked_speed_sources(&broker, &namespace, &VEHICLE_BRAND_PARKED_SPEED_SOURCE_TERMS).await?;
    let id = namespace.urn("Vehicle", 1)?;
    let fragment = EntityFragment::builder()
        .property("speed", speedometer_speed_56(&namespace, "speedometerA4567-speed2")?)
        .property("brandName", "BMW")
        .build()?;
    let client = ld_json_body_client(&broker, &vocabulary)?;
    let operation = AppendAttributes::builder().id(&id).fragment(&fragment).build();
    let answer = client.send_unchecked(with_doubled_separator(client.unchecked(&operation)?)).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest).with_title());
    delete_entities(&broker, &[&id]).await;
    Ok(())
}
