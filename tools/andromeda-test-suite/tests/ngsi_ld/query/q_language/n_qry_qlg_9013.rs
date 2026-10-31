//! A vocab property matches a uri in its vocab.

use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    problem::answered,
    q_selection::selected_count,
    room_building::{RoomBuilding, RoomInstants},
};
use googletest::prelude::*;
use http::StatusCode;

/// Pins: Query Entities with q equal to an IRI a `VocabProperty` holds selects the Entity, and to another IRI does
/// not
/// Clause: 4.9
#[gtest]
#[tokio::test]
async fn n_qry_qlg_9013_a_vocab_property_matches_a_uri_in_its_vocab() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", "a-vocab-property-matches-a-uri-in-its-vo")?;
    let building_type = namespace.entity_type("Building")?;
    let building = RoomBuilding::new(&building_id, &building_type, &namespace, RoomInstants::Strings)?;
    // The model writes no plain-string `objectList` and no several-string `languageMap` entry, so the Building is sent as a document.
    let answer = broker.send_unchecked(building.create_request(&broker)?).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    expect_that!(selected_count(&broker, &building_id, r#"roomVocab=="http://example.org/Other""#).await?, eq(1));
    expect_that!(selected_count(&broker, &building_id, r#"roomVocab=="http://example.org/Male""#).await?, eq(1));
    expect_that!(selected_count(&broker, &building_id, r#"roomVocab=="http://example.org/Female""#).await?, eq(0));
    Ok(())
}
