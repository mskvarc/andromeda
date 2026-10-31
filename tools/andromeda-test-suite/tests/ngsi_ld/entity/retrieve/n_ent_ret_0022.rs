//! Key values renders a list relationship as the array of its objects.

use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    problem::answered,
    simplified_matcher::{has_length_at, has_value_at},
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{
        UncheckedDocument,
        headers::Accept,
        operation::{CreateEntity, RepresentationSpelling},
    },
    model::{Entity, Lossless, NgsiLdMediaType, RawJson},
};

/// Pins: Retrieve Entity with `options=keyValues` answers a `ListRelationship` as the bare array of its object
/// URIs, in order
/// Clause: 4.5.4, 4.5.22, 5.7.1
#[gtest]
#[tokio::test]
async fn n_ent_ret_0022_key_values_renders_a_list_relationship_as_the_array_of_its_objects() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building = Entity::builder().id(building_id.clone()).entity_type(building_type).build()?;
    // The model refuses it (the model reads it as written otherwise [at $.hasRooms.objectList[0] (written "urn:ngsi-ld:Room:zqns-X")]: {"id":"urn:ngsi-ld:Building:zqns-1","type":"Building_zqns","hasRooms":{"type":"ListRelationship","objectList":[{"object":"urn:ngsi-ld:Room:zqns-X"},{"object":"urn:ngsi-ld:Room:zqns-Y"}]}}), so `hasRooms` is written as text.
    let has_rooms: RawJson =
        format!(r#"{{"type": "ListRelationship", "objectList": ["urn:ngsi-ld:Room:{namespace}-X", "urn:ngsi-ld:Room:{namespace}-Y"]}}"#).parse()?;
    let document = UncheckedDocument::from_entity(&building, Lossless::Normalized).set_member("hasRooms", &has_rooms);
    let request = broker.unchecked(&CreateEntity::builder().entity(&building).build())?.document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve_simplified(&building_id)
        .spelling(RepresentationSpelling::default().in_options().key_values())
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_length_at("hasRooms", [], 2));
    expect_that!(entity, has_value_at("hasRooms", ["0"], format!("urn:ngsi-ld:Room:{namespace}-X")));
    expect_that!(entity, has_value_at("hasRooms", ["1"], format!("urn:ngsi-ld:Room:{namespace}-Y")));
    Ok(())
}
