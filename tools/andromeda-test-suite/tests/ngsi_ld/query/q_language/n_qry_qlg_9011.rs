//! An unquoted fractional instant matches a typed value list member and no string.

use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    problem::answered,
    q_selection::selected_count,
    room_building::{RoomBuilding, RoomInstants},
};
use googletest::prelude::*;
use http::StatusCode;

/// Pins: Query Entities with q equal to the unquoted `dateTime` 2024-01-01T00:00:00.250Z selects the Entity through
/// a `valueList` member typed `DateTime` and holding that instant, and not through a `valueList` member that is the
/// untyped string of the same text
/// Clause: 4.9, 4.6.3, C.6
#[gtest]
#[tokio::test]
async fn n_qry_qlg_9011_an_unquoted_fractional_instant_matches_a_typed_value_list_member_and_no_string() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", "an-unquoted-fractional-instant-matches-a")?;
    let building_type = namespace.entity_type("Building")?;
    let building = RoomBuilding::new(&building_id, &building_type, &namespace, RoomInstants::Typed)?;
    // The model writes no plain-string `objectList` and no several-string `languageMap` entry, so the Building is sent as a document.
    let answer = broker.send_unchecked(building.create_request(&broker)?).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    expect_that!(selected_count(&broker, &building_id, "roomInstants==2024-01-01T00:00:00.250Z").await?, eq(1));
    expect_that!(
        selected_count(&broker, &building_id, "roomInstantLabels==2024-01-01T00:00:00.250Z").await?,
        eq(0)
    );
    expect_that!(
        selected_count(&broker, &building_id, r#"roomInstantLabels=="2024-01-01T00:00:00.250Z""#).await?,
        eq(1)
    );
    Ok(())
}
