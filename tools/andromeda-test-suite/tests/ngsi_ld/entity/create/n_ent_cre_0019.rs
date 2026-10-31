//! A list relationship to entities that do not exist is kept whole.

use andromeda_test_suite::{
    attribute_matcher::attribute_of,
    broker,
    entity_matcher::{attribute_members, has_attribute},
    namespace::Namespace,
    problem::answered,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::CreateEntity},
    model::{AttributeKind, Entity, Lossless, NgsiLdMediaType, RawJson},
};

/// Pins: a `ListRelationship` whose `objectList` names entities that do not exist is created and reads back with
/// the whole `objectList`
/// Clause: 4.5.22, 5.6.1, 5.7.1
#[gtest]
#[tokio::test]
async fn n_ent_cre_0019_a_list_relationship_to_entities_that_do_not_exist_is_kept_whole() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let room_non_existent1_id = namespace.urn("Room", "NonExistent1")?;
    let room_non_existent2_id = namespace.urn("Room", "NonExistent2")?;
    let building = Entity::builder().id(building_id.clone()).entity_type(building_type).build()?;
    // The model refuses it (the model reads it as written otherwise [at $.hasRooms.objectList[0] (written "urn:ngsi-ld:Room:zqns-NonExistent1")]: {"id":"urn:ngsi-ld:Building:zqns-1","type":"Building_zqns","hasRooms":{"type":"ListRelationship","objectList":[{"object":"urn:ngsi-ld:Room:zqns-NonExistent1"},{"object":"urn:ngsi-ld:Room:zqns-NonExistent2"}]}}), so `hasRooms` is written as text.
    let has_rooms: RawJson =
        format!(r#"{{"type": "ListRelationship", "objectList": ["urn:ngsi-ld:Room:{namespace}-NonExistent1", "urn:ngsi-ld:Room:{namespace}-NonExistent2"]}}"#)
            .parse()?;
    let document = UncheckedDocument::from_entity(&building, Lossless::Normalized).set_member("hasRooms", &has_rooms);
    let request = broker.unchecked(&CreateEntity::builder().entity(&building).build())?.document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(attribute_members(entity, "hasRooms"), some(eq(2)));
    expect_that!(
        entity,
        has_attribute(
            "hasRooms",
            attribute_of(AttributeKind::ListRelationship)
                .list_length(2)
                .list_object(0, room_non_existent1_id)
                .list_object(1, room_non_existent2_id)
        )
    );
    Ok(())
}
