//! A term without a language matches any language.

use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    problem::answered,
    q_selection::selected_count,
    room_building::{RoomBuilding, RoomInstants},
};
use googletest::prelude::*;
use http::StatusCode;

/// Pins: Query Entities with q roomLabel=="Tuer", roomLabel=="Door" and roomLabel`[*]`=="Tuer" selects the Entity,
/// and roomLabel=="rot" does not
/// Clause: 4.9
#[gtest]
#[tokio::test]
async fn n_qry_qlg_9007_a_term_without_a_language_matches_any_language() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", "a-term-without-a-language-matches-any-la")?;
    let building_type = namespace.entity_type("Building")?;
    let building = RoomBuilding::new(&building_id, &building_type, &namespace, RoomInstants::Strings)?;
    // The model writes no plain-string `objectList` and no several-string `languageMap` entry, so the Building is sent as a document.
    let answer = broker.send_unchecked(building.create_request(&broker)?).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    expect_that!(selected_count(&broker, &building_id, r#"roomLabel=="Tuer""#).await?, eq(1));
    expect_that!(selected_count(&broker, &building_id, r#"roomLabel=="Door""#).await?, eq(1));
    expect_that!(selected_count(&broker, &building_id, r#"roomLabel[*]=="Tuer""#).await?, eq(1));
    expect_that!(selected_count(&broker, &building_id, r#"roomLabel=="rot""#).await?, eq(0));
    Ok(())
}
