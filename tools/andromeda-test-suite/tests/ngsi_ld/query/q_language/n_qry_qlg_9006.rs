//! A language selected term matches that language alone.

use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    problem::answered,
    q_selection::selected_count,
    room_building::{RoomBuilding, RoomInstants},
};
use googletest::prelude::*;
use http::StatusCode;

/// Pins: Query Entities with q roomLabel`[en]`=="Door" and roomLabel`[de]`=="Tuer" selects the Entity, and
/// roomLabel`[de]`=="Door" and roomLabel`[en]`=="Tuer" do not
/// Clause: 4.9
#[gtest]
#[tokio::test]
async fn n_qry_qlg_9006_a_language_selected_term_matches_that_language_alone() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", "a-language-selected-term-matches-that-la")?;
    let building_type = namespace.entity_type("Building")?;
    let building = RoomBuilding::new(&building_id, &building_type, &namespace, RoomInstants::Strings)?;
    // The model writes no plain-string `objectList` and no several-string `languageMap` entry, so the Building is sent as a document.
    let answer = broker.send_unchecked(building.create_request(&broker)?).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    expect_that!(selected_count(&broker, &building_id, r#"roomLabel[en]=="Door""#).await?, eq(1));
    expect_that!(selected_count(&broker, &building_id, r#"roomLabel[de]=="Tuer""#).await?, eq(1));
    expect_that!(selected_count(&broker, &building_id, r#"roomLabel[de]=="Door""#).await?, eq(0));
    expect_that!(selected_count(&broker, &building_id, r#"roomLabel[en]=="Tuer""#).await?, eq(0));
    Ok(())
}
