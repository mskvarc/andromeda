//! A mistyped term is unequal to a scalar property.

use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    problem::answered,
    q_selection::selected_count,
    room_building::{RoomBuilding, RoomInstants},
};
use googletest::prelude::*;
use http::StatusCode;

/// Pins: Query Entities over a number, a string and a `valueList` of strings selects the Entity for != and not for
/// == whenever the Query Term value is of another data type
/// Clause: 4.9
#[gtest]
#[tokio::test]
async fn n_qry_qlg_9020_a_mistyped_term_is_unequal_to_a_scalar_property() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", "a-mistyped-term-is-unequal-to-a-scalar-p")?;
    let building_type = namespace.entity_type("Building")?;
    let building = RoomBuilding::new(&building_id, &building_type, &namespace, RoomInstants::Strings)?;
    // The model writes no plain-string `objectList` and no several-string `languageMap` entry, so the Building is sent as a document.
    let answer = broker.send_unchecked(building.create_request(&broker)?).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    expect_that!(selected_count(&broker, &building_id, "roomTemp==20").await?, eq(1));
    expect_that!(selected_count(&broker, &building_id, "roomTemp!=20").await?, eq(0));
    expect_that!(selected_count(&broker, &building_id, r#"roomTemp!="20""#).await?, eq(1));
    expect_that!(selected_count(&broker, &building_id, r#"roomTemp=="20""#).await?, eq(0));
    expect_that!(selected_count(&broker, &building_id, "roomTemp!=true").await?, eq(1));
    expect_that!(selected_count(&broker, &building_id, "roomTemp==true").await?, eq(0));
    expect_that!(selected_count(&broker, &building_id, r#"roomTemp!="2020-01-01T00:00:00Z""#).await?, eq(1));
    expect_that!(selected_count(&broker, &building_id, r#"roomTemp=="2020-01-01T00:00:00Z""#).await?, eq(0));
    expect_that!(selected_count(&broker, &building_id, "roomName!=5").await?, eq(1));
    expect_that!(selected_count(&broker, &building_id, "roomName==5").await?, eq(0));
    expect_that!(selected_count(&broker, &building_id, r#"roomName!="atrium""#).await?, eq(0));
    expect_that!(selected_count(&broker, &building_id, r#"roomName!="foyer""#).await?, eq(1));
    expect_that!(selected_count(&broker, &building_id, "roomTags!=5").await?, eq(1));
    expect_that!(selected_count(&broker, &building_id, "roomTags==5").await?, eq(0));
    Ok(())
}
