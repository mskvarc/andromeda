//! A list relationship reads back with its object list.

use andromeda_test_suite::{
    attribute_matcher::attribute_of,
    broker,
    entity_matcher::{attribute_members, has_attribute},
    namespace::Namespace,
    outcome::Done,
    problem::answered,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::CreateEntity},
    model::{AttributeKind, Entity, Lossless, NgsiLdMediaType, RawJson},
};

/// Pins: a `ListRelationship` reads back in the normalized representation with its type and an `objectList` of
/// Relationship objects naming its targets in order
/// Clause: 4.5.22.2, 5.6.1, 5.7.1
#[gtest]
#[tokio::test]
async fn n_ent_cre_0018_a_list_relationship_reads_back_with_its_object_list() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let room_a_id = namespace.urn("Room", "A")?;
    let room_type = namespace.entity_type("Room")?;
    let room_b_id = namespace.urn("Room", "B")?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let entities = [
        Entity::builder()
            .id(room_a_id.clone())
            .entity_type(room_type.clone())
            .property("name", "Room A")
            .build()?,
        Entity::builder()
            .id(room_b_id.clone())
            .entity_type(room_type.clone())
            .property("name", "Room B")
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    let building = Entity::builder().id(building_id.clone()).entity_type(building_type).build()?;
    // The model refuses it (the model reads it as written otherwise [at $.hasRooms.objectList[0] (written "urn:ngsi-ld:Room:zqns-A")]: {"id":"urn:ngsi-ld:Building:zqns-1","type":"Building_zqns","hasRooms":{"type":"ListRelationship","objectList":[{"object":"urn:ngsi-ld:Room:zqns-A"},{"object":"urn:ngsi-ld:Room:zqns-B"}]}}), so `hasRooms` is written as text.
    let has_rooms: RawJson =
        format!(r#"{{"type": "ListRelationship", "objectList": ["urn:ngsi-ld:Room:{namespace}-A", "urn:ngsi-ld:Room:{namespace}-B"]}}"#).parse()?;
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
                .list_object(0, room_a_id.clone())
                .list_object(1, room_b_id.clone())
        )
    );
    Ok(())
}
