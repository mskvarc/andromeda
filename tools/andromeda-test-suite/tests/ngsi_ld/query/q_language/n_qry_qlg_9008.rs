//! A language array value matches a single element.

use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    problem::answered,
    q_selection::selected_count,
    room_building::{RoomBuilding, RoomInstants},
};
use googletest::prelude::*;
use http::StatusCode;

/// Pins: Query Entities with q on a `languageMap` holding arrays selects the Entity for an element of the named
/// language or of any language, and not for an element of another language
/// Clause: 4.9
#[gtest]
#[tokio::test]
async fn n_qry_qlg_9008_a_language_array_value_matches_a_single_element() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", "a-language-array-value-matches-a-single-")?;
    let building_type = namespace.entity_type("Building")?;
    let building = RoomBuilding::new(&building_id, &building_type, &namespace, RoomInstants::Strings)?;
    // The model writes no plain-string `objectList` and no several-string `languageMap` entry, so the Building is sent as a document.
    let answer = broker.send_unchecked(building.create_request(&broker)?).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    expect_that!(selected_count(&broker, &building_id, r#"roomAliases[en]=="cat""#).await?, eq(1));
    expect_that!(selected_count(&broker, &building_id, r#"roomAliases[fr]=="rouge""#).await?, eq(1));
    expect_that!(selected_count(&broker, &building_id, r#"roomAliases=="chat""#).await?, eq(1));
    expect_that!(selected_count(&broker, &building_id, r#"roomAliases[en]=="chat""#).await?, eq(0));
    Ok(())
}
