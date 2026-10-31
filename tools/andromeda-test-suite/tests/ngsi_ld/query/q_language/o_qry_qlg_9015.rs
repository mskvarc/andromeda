//! Unequal on a language the map does not hold selects the entity.

use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    problem::answered,
    q_selection::selected_count,
    room_building::{RoomBuilding, RoomInstants},
};
use googletest::prelude::*;
use http::StatusCode;

/// Pins: Query Entities with q roomLabel`[xx]`!="Door" over a `languageMap` holding no xx key selects the Entity
/// Clause: 4.9
/// Recorded: 4.9 asks a matching Entity to contain the target element and, for unequal with a target language, that
/// no matching value is found under that key; whether a key the `languageMap` does not hold leaves the
/// `LanguageProperty` the target element is left open. Andromeda selects the Entity, while the existence term
/// roomLabel`[xx]` selects nothing
#[gtest]
#[tokio::test]
async fn o_qry_qlg_9015_unequal_on_a_language_the_map_does_not_hold_selects_the_entity() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", "unequal-on-a-language-the-map-does-not-h")?;
    let building_type = namespace.entity_type("Building")?;
    let building = RoomBuilding::new(&building_id, &building_type, &namespace, RoomInstants::Strings)?;
    // The model writes no plain-string `objectList` and no several-string `languageMap` entry, so the Building is sent as a document.
    let answer = broker.send_unchecked(building.create_request(&broker)?).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    expect_that!(selected_count(&broker, &building_id, r#"roomLabel[xx]!="Door""#).await?, eq(1));
    Ok(())
}
