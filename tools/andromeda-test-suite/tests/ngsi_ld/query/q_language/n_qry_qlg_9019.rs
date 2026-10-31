//! Existence of a language selected term needs that key.

use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    problem::answered,
    q_selection::selected_count,
    room_building::{RoomBuilding, RoomInstants},
};
use googletest::prelude::*;
use http::StatusCode;

/// Pins: Query Entities with q `roomLabel` and roomLabel`[en]` selects the Entity, and roomLabel`[xx]` does not
/// Clause: 4.9
#[gtest]
#[tokio::test]
async fn n_qry_qlg_9019_existence_of_a_language_selected_term_needs_that_key() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", "existence-of-a-language-selected-term-ne")?;
    let building_type = namespace.entity_type("Building")?;
    let building = RoomBuilding::new(&building_id, &building_type, &namespace, RoomInstants::Strings)?;
    // The model writes no plain-string `objectList` and no several-string `languageMap` entry, so the Building is sent as a document.
    let answer = broker.send_unchecked(building.create_request(&broker)?).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    expect_that!(selected_count(&broker, &building_id, "roomLabel").await?, eq(1));
    expect_that!(selected_count(&broker, &building_id, "roomLabel[en]").await?, eq(1));
    expect_that!(selected_count(&broker, &building_id, "roomLabel[xx]").await?, eq(0));
    Ok(())
}
