//! An unquoted instant does not match a value list string.

use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    problem::answered,
    q_selection::selected_count,
    room_building::{RoomBuilding, RoomInstants},
};
use googletest::prelude::*;
use http::StatusCode;

/// Pins: Query Entities with q equal to the unquoted dateTime of a `valueList` member written as a string with a
/// fraction selects nothing, and the same instant quoted as written selects the Entity
/// Clause: 4.9, 4.6.3, C.6
/// Recorded: a value is a time value only when its `valueType` declares it one (annex C.6); a `valueList` string
/// declares none, so it is a String, which a bare `DateTime` operand does not match (clause 4.9) and a quoted operand
/// matches when it is included in the list
#[gtest]
#[tokio::test]
async fn o_qry_qlg_9011_an_unquoted_instant_does_not_match_a_value_list_string() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", "an-unquoted-fractional-instant-matches-a")?;
    let building_type = namespace.entity_type("Building")?;
    let building = RoomBuilding::new(&building_id, &building_type, &namespace, RoomInstants::Strings)?;
    // The model writes no plain-string `objectList` and no several-string `languageMap` entry, so the Building is sent as a document.
    let answer = broker.send_unchecked(building.create_request(&broker)?).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    expect_that!(selected_count(&broker, &building_id, "roomInstants==2024-01-01T00:00:00.250Z").await?, eq(0));
    expect_that!(
        selected_count(&broker, &building_id, r#"roomInstants=="2024-01-01T00:00:00.250Z""#).await?,
        eq(1)
    );
    Ok(())
}
