//! Unequal is the complement over every structured target.

use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    problem::answered,
    q_selection::selected_count,
    room_building::{RoomBuilding, RoomInstants},
};
use googletest::prelude::*;
use http::StatusCode;

/// Pins: Query Entities with != over a language-selected value, a `languageMap`, a language array, a `valueList`,
/// an `objectList` and a vocab selects the Entity only when the value is not held
/// Clause: 4.9
#[gtest]
#[tokio::test]
async fn n_qry_qlg_9014_unequal_is_the_complement_over_every_structured_target() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", "unequal-is-the-complement-over-every-str")?;
    let building_type = namespace.entity_type("Building")?;
    let building = RoomBuilding::new(&building_id, &building_type, &namespace, RoomInstants::Strings)?;
    // The model writes no plain-string `objectList` and no several-string `languageMap` entry, so the Building is sent as a document.
    let answer = broker.send_unchecked(building.create_request(&broker)?).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    expect_that!(selected_count(&broker, &building_id, r#"roomLabel[en]!="Tuer""#).await?, eq(1));
    expect_that!(selected_count(&broker, &building_id, r#"roomLabel[en]!="Door""#).await?, eq(0));
    expect_that!(selected_count(&broker, &building_id, r#"roomLabel!="rot""#).await?, eq(1));
    expect_that!(selected_count(&broker, &building_id, r#"roomLabel!="Tuer""#).await?, eq(0));
    expect_that!(selected_count(&broker, &building_id, r#"roomAliases[en]!="chat""#).await?, eq(1));
    expect_that!(selected_count(&broker, &building_id, r#"roomAliases[en]!="cat""#).await?, eq(0));
    expect_that!(selected_count(&broker, &building_id, r#"roomTags!="industrial""#).await?, eq(1));
    expect_that!(selected_count(&broker, &building_id, r#"roomTags!="commercial""#).await?, eq(0));
    expect_that!(
        selected_count(&broker, &building_id, &format!(r#"roomLinks!="urn:ngsi-ld:Room:{namespace}-9""#)).await?,
        eq(1)
    );
    expect_that!(
        selected_count(&broker, &building_id, &format!(r#"roomLinks!="urn:ngsi-ld:Room:{namespace}-2""#)).await?,
        eq(0)
    );
    expect_that!(selected_count(&broker, &building_id, r#"roomVocab!="http://example.org/Female""#).await?, eq(1));
    expect_that!(selected_count(&broker, &building_id, r#"roomVocab!="http://example.org/Male""#).await?, eq(0));
    Ok(())
}
