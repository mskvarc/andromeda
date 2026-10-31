//! A datatype mismatch fails equal and satisfies unequal.

use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    problem::answered,
    q_selection::selected_count,
    room_building::{RoomBuilding, RoomInstants},
};
use googletest::prelude::*;
use http::StatusCode;

/// Pins: Query Entities over a `valueList` of numbers selects the Entity for ==2 and !="2" and !=true, and not for
/// =="2", !=2 or ==true
/// Clause: 4.9
#[gtest]
#[tokio::test]
async fn n_qry_qlg_9018_a_datatype_mismatch_fails_equal_and_satisfies_unequal() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", "a-datatype-mismatch-fails-equal-and-sati")?;
    let building_type = namespace.entity_type("Building")?;
    let building = RoomBuilding::new(&building_id, &building_type, &namespace, RoomInstants::Strings)?;
    // The model writes no plain-string `objectList` and no several-string `languageMap` entry, so the Building is sent as a document.
    let answer = broker.send_unchecked(building.create_request(&broker)?).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    expect_that!(selected_count(&broker, &building_id, "roomSpeeds==2").await?, eq(1));
    expect_that!(selected_count(&broker, &building_id, r#"roomSpeeds=="2""#).await?, eq(0));
    expect_that!(selected_count(&broker, &building_id, r#"roomSpeeds!="2""#).await?, eq(1));
    expect_that!(selected_count(&broker, &building_id, "roomSpeeds!=2").await?, eq(0));
    expect_that!(selected_count(&broker, &building_id, "roomSpeeds==true").await?, eq(0));
    expect_that!(selected_count(&broker, &building_id, "roomSpeeds!=true").await?, eq(1));
    Ok(())
}
