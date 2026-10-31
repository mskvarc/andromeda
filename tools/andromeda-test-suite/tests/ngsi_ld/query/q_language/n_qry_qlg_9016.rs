//! A value list term reaches every structured target.

use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    problem::answered,
    q_selection::selected_count,
    room_building::{RoomBuilding, RoomInstants},
};
use googletest::prelude::*;
use http::StatusCode;

/// Pins: Query Entities with == and a list of values selects the Entity when any listed value is held by the
/// `languageMap`, `valueList`, `objectList` or vocab, and not when none is
/// Clause: 4.9
#[gtest]
#[tokio::test]
async fn n_qry_qlg_9016_a_value_list_term_reaches_every_structured_target() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", "a-value-list-term-reaches-every-structur")?;
    let building_type = namespace.entity_type("Building")?;
    let building = RoomBuilding::new(&building_id, &building_type, &namespace, RoomInstants::Strings)?;
    // The model writes no plain-string `objectList` and no several-string `languageMap` entry, so the Building is sent as a document.
    let answer = broker.send_unchecked(building.create_request(&broker)?).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    expect_that!(selected_count(&broker, &building_id, r#"roomLabel=="nothing","Tuer""#).await?, eq(1));
    expect_that!(selected_count(&broker, &building_id, r#"roomLabel[en]=="nothing","Door""#).await?, eq(1));
    expect_that!(selected_count(&broker, &building_id, r#"roomAliases=="nothing","chat""#).await?, eq(1));
    expect_that!(selected_count(&broker, &building_id, r#"roomTags=="industrial","commercial""#).await?, eq(1));
    expect_that!(selected_count(&broker, &building_id, "roomSpeeds==8,2").await?, eq(1));
    expect_that!(
        selected_count(
            &broker,
            &building_id,
            &format!(r#"roomLinks=="urn:ngsi-ld:Room:{namespace}-9","urn:ngsi-ld:Room:{namespace}-2""#)
        )
        .await?,
        eq(1)
    );
    expect_that!(
        selected_count(&broker, &building_id, r#"roomVocab=="http://example.org/Female","http://example.org/Male""#).await?,
        eq(1)
    );
    expect_that!(selected_count(&broker, &building_id, r#"roomTags=="industrial","agricultural""#).await?, eq(0));
    expect_that!(selected_count(&broker, &building_id, "roomSpeeds==8,9").await?, eq(0));
    Ok(())
}
