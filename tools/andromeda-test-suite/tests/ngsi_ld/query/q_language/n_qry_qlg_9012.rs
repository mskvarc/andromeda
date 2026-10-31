//! A list relationship matches a uri in its object list.

use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    problem::answered,
    q_selection::selected_count,
    room_building::{RoomBuilding, RoomInstants},
};
use googletest::prelude::*;
use http::StatusCode;

/// Pins: Query Entities with q equal to a URI of an `objectList` selects the Entity, and to another URI does not
/// Clause: 4.9
#[gtest]
#[tokio::test]
async fn n_qry_qlg_9012_a_list_relationship_matches_a_uri_in_its_object_list() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", "a-list-relationship-matches-a-uri-in-its")?;
    let building_type = namespace.entity_type("Building")?;
    let building = RoomBuilding::new(&building_id, &building_type, &namespace, RoomInstants::Strings)?;
    // The model writes no plain-string `objectList` and no several-string `languageMap` entry, so the Building is sent as a document.
    let answer = broker.send_unchecked(building.create_request(&broker)?).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    expect_that!(
        selected_count(&broker, &building_id, &format!(r#"roomLinks=="urn:ngsi-ld:Room:{namespace}-2""#)).await?,
        eq(1)
    );
    expect_that!(
        selected_count(&broker, &building_id, &format!(r#"roomLinks=="urn:ngsi-ld:Room:{namespace}-1""#)).await?,
        eq(1)
    );
    expect_that!(
        selected_count(&broker, &building_id, &format!(r#"roomLinks=="urn:ngsi-ld:Room:{namespace}-9""#)).await?,
        eq(0)
    );
    Ok(())
}
