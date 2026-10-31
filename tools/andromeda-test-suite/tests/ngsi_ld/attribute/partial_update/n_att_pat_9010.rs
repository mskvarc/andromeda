//! An object list of one null deletes the list relationship.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::{has_attribute, lacks_attribute},
    namespace::Namespace,
    outcome::Done,
    problem::answered,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::CreateEntity},
    model::{AttributeKind, AttributeName, Change, Entity, InstanceDeletion, ListProperty, Lossless, NgsiLdMediaType, RawJson, Value},
};

/// Pins: Partial Attribute Update of a `ListRelationship` with `objectList` `["urn:ngsi-ld:null"]` answers 204 and
/// deletes the `ListRelationship`
/// Clause: 4.5.22.2, 4.5.0, 5.5.8, 5.6.4
#[gtest]
#[tokio::test]
async fn n_att_pat_9010_an_object_list_of_one_null_deletes_the_list_relationship() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_object_list_null_id = namespace.urn("Building", "object-list-null")?;
    let building_type = namespace.entity_type("Building")?;
    let building_object_list_null = Entity::builder()
        .id(building_object_list_null_id.clone())
        .entity_type(building_type)
        .list_property("tags", ListProperty::builder(vec![Value::from("residential"), Value::from("commercial")]))
        .build()?;
    // The model refuses it (the model reads it as written otherwise [at $.hasRooms.objectList[0] (written "urn:ngsi-ld:Room:zqns-1")]: {"id":"urn:ngsi-ld:Building:zqns-object-list-null","type":"Building_zqns","tags":{"type":"ListProperty","valueList":["residential","commercial"]},"hasRooms":{"type":"ListRelationship","objectList":[{"object":"urn:ngsi-ld:Room:zqns-1"},{"object":"urn:ngsi-ld:Room:zqns-2"}]}}), so `hasRooms` is written as text.
    let has_rooms: RawJson =
        format!(r#"{{"type": "ListRelationship", "objectList": ["urn:ngsi-ld:Room:{namespace}-1", "urn:ngsi-ld:Room:{namespace}-2"]}}"#).parse()?;
    let document = UncheckedDocument::from_entity(&building_object_list_null, Lossless::Normalized).set_member("hasRooms", &has_rooms);
    let request = broker
        .unchecked(&CreateEntity::builder().entity(&building_object_list_null).build())?
        .document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    let deletion = Change::Delete(InstanceDeletion::new(AttributeKind::ListRelationship));
    broker
        .entities()
        .update_attribute(&building_object_list_null_id, &"hasRooms".parse::<AttributeName>()?, deletion)
        .send()
        .await?
        .done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_object_list_null_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, lacks_attribute("hasRooms"));
    expect_that!(entity, has_attribute("tags", any_attribute()));
    Ok(())
}
