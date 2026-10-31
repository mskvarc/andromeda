//! A mistyped range is unequal to a scalar property.

use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    problem::answered,
    q_selection::selected_count,
    room_building::{RoomBuilding, RoomInstants},
};
use googletest::prelude::*;
use http::StatusCode;

/// Pins: Query Entities with a numeric range selects a number inside it for == and outside it for !=, and a string
/// for != and not for ==
/// Clause: 4.9
#[gtest]
#[tokio::test]
async fn n_qry_qlg_9021_a_mistyped_range_is_unequal_to_a_scalar_property() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", "a-mistyped-range-is-unequal-to-a-scalar-")?;
    let building_type = namespace.entity_type("Building")?;
    let building = RoomBuilding::new(&building_id, &building_type, &namespace, RoomInstants::Strings)?;
    // The model writes no plain-string `objectList` and no several-string `languageMap` entry, so the Building is sent as a document.
    let answer = broker.send_unchecked(building.create_request(&broker)?).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    expect_that!(selected_count(&broker, &building_id, "roomTemp==10..30").await?, eq(1));
    expect_that!(selected_count(&broker, &building_id, "roomTemp!=10..30").await?, eq(0));
    expect_that!(selected_count(&broker, &building_id, "roomTemp!=25..30").await?, eq(1));
    expect_that!(selected_count(&broker, &building_id, "roomName!=10..30").await?, eq(1));
    expect_that!(selected_count(&broker, &building_id, "roomName==10..30").await?, eq(0));
    Ok(())
}
