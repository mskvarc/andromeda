//! A negated value list term denies every member.

use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    problem::answered,
    q_selection::selected_count,
    room_building::{RoomBuilding, RoomInstants},
};
use googletest::prelude::*;
use http::StatusCode;

/// Pins: Query Entities with != and a list of values selects the Entity when the target holds none of them, and not
/// when it holds one
/// Clause: 4.9
#[gtest]
#[tokio::test]
async fn n_qry_qlg_9017_a_negated_value_list_term_denies_every_member() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", "a-negated-value-list-term-denies-every-m")?;
    let building_type = namespace.entity_type("Building")?;
    let building = RoomBuilding::new(&building_id, &building_type, &namespace, RoomInstants::Strings)?;
    // The model writes no plain-string `objectList` and no several-string `languageMap` entry, so the Building is sent as a document.
    let answer = broker.send_unchecked(building.create_request(&broker)?).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    expect_that!(selected_count(&broker, &building_id, r#"roomTags!="industrial","agricultural""#).await?, eq(1));
    expect_that!(selected_count(&broker, &building_id, r#"roomTags!="industrial","commercial""#).await?, eq(0));
    expect_that!(
        selected_count(
            &broker,
            &building_id,
            &format!(r#"roomLinks!="urn:ngsi-ld:Room:{namespace}-8","urn:ngsi-ld:Room:{namespace}-9""#)
        )
        .await?,
        eq(1)
    );
    expect_that!(
        selected_count(
            &broker,
            &building_id,
            &format!(r#"roomLinks!="urn:ngsi-ld:Room:{namespace}-9","urn:ngsi-ld:Room:{namespace}-2""#)
        )
        .await?,
        eq(0)
    );
    expect_that!(
        selected_count(&broker, &building_id, r#"roomVocab!="http://example.org/Female","http://example.org/Nobody""#).await?,
        eq(1)
    );
    expect_that!(
        selected_count(&broker, &building_id, r#"roomVocab!="http://example.org/Female","http://example.org/Male""#).await?,
        eq(0)
    );
    expect_that!(selected_count(&broker, &building_id, r#"roomLabel!="rot","rouge""#).await?, eq(1));
    expect_that!(selected_count(&broker, &building_id, r#"roomLabel!="rot","Tuer""#).await?, eq(0));
    Ok(())
}
