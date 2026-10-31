//! A list property matches a member of its value list.

use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    problem::answered,
    q_selection::selected_count,
    room_building::{RoomBuilding, RoomInstants},
};
use googletest::prelude::*;
use http::StatusCode;

/// Pins: Query Entities with q equal to a member of a `valueList` of strings or numbers selects the Entity, and to
/// a value the list does not hold does not
/// Clause: 4.9
#[gtest]
#[tokio::test]
async fn n_qry_qlg_9009_a_list_property_matches_a_member_of_its_value_list() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", "a-list-property-matches-a-member-of-its-")?;
    let building_type = namespace.entity_type("Building")?;
    let building = RoomBuilding::new(&building_id, &building_type, &namespace, RoomInstants::Strings)?;
    // The model writes no plain-string `objectList` and no several-string `languageMap` entry, so the Building is sent as a document.
    let answer = broker.send_unchecked(building.create_request(&broker)?).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    expect_that!(selected_count(&broker, &building_id, r#"roomTags=="commercial""#).await?, eq(1));
    expect_that!(selected_count(&broker, &building_id, r#"roomTags=="residential""#).await?, eq(1));
    expect_that!(selected_count(&broker, &building_id, "roomSpeeds==2").await?, eq(1));
    expect_that!(selected_count(&broker, &building_id, r#"roomTags=="industrial""#).await?, eq(0));
    expect_that!(selected_count(&broker, &building_id, "roomSpeeds==9").await?, eq(0));
    Ok(())
}
