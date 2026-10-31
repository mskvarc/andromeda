//! A quoted fractional instant matches a value list member.

use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    problem::answered,
    q_selection::selected_count,
    room_building::{RoomBuilding, RoomInstants},
};
use googletest::prelude::*;
use http::StatusCode;

/// Pins: Query Entities with q equal to the quoted text of a `valueList` member holding a `DateTime` or Time with a
/// fraction selects the Entity, and to another fraction does not
/// Clause: 4.9, 4.6.3
#[gtest]
#[tokio::test]
async fn n_qry_qlg_9010_a_quoted_fractional_instant_matches_a_value_list_member() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", "a-quoted-fractional-instant-matches-a-va")?;
    let building_type = namespace.entity_type("Building")?;
    let building = RoomBuilding::new(&building_id, &building_type, &namespace, RoomInstants::Strings)?;
    // The model writes no plain-string `objectList` and no several-string `languageMap` entry, so the Building is sent as a document.
    let answer = broker.send_unchecked(building.create_request(&broker)?).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    expect_that!(
        selected_count(&broker, &building_id, r#"roomInstants=="2024-01-01T00:00:00.250Z""#).await?,
        eq(1)
    );
    expect_that!(selected_count(&broker, &building_id, r#"roomInstants=="2024-01-02T00:00:00Z""#).await?, eq(1));
    expect_that!(
        selected_count(&broker, &building_id, r#"roomInstants=="2024-01-01T00:00:00.750Z""#).await?,
        eq(0)
    );
    expect_that!(selected_count(&broker, &building_id, r#"roomTimes=="09:30:00.250Z""#).await?, eq(1));
    expect_that!(selected_count(&broker, &building_id, r#"roomTimes=="13:00:00Z""#).await?, eq(1));
    expect_that!(selected_count(&broker, &building_id, r#"roomTimes=="09:30:00.750Z""#).await?, eq(0));
    Ok(())
}
