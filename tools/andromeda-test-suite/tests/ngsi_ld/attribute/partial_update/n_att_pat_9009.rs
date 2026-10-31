//! A value list of one null deletes the list property.

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

/// Pins: Partial Attribute Update of a `ListProperty` with `valueList` `["urn:ngsi-ld:null"]` answers 204 and
/// deletes the `ListProperty`
/// Clause: 4.5.21.2, 4.5.0, 5.5.8, 5.6.4
#[gtest]
#[tokio::test]
async fn n_att_pat_9009_a_value_list_of_one_null_deletes_the_list_property() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_value_list_null_id = namespace.urn("Building", "value-list-null")?;
    let building_type = namespace.entity_type("Building")?;
    let building_value_list_null = Entity::builder()
        .id(building_value_list_null_id.clone())
        .entity_type(building_type)
        .list_property("tags", ListProperty::builder(vec![Value::from("residential"), Value::from("commercial")]))
        .build()?;
    // The model refuses it (the model reads it as written otherwise [at $.hasRooms.objectList[0] (written "urn:ngsi-ld:Room:zqns-1")]: {"id":"urn:ngsi-ld:Building:zqns-value-list-null","type":"Building_zqns","tags":{"type":"ListProperty","valueList":["residential","commercial"]},"hasRooms":{"type":"ListRelationship","objectList":[{"object":"urn:ngsi-ld:Room:zqns-1"},{"object":"urn:ngsi-ld:Room:zqns-2"}]}}), so `hasRooms` is written as text.
    let has_rooms: RawJson =
        format!(r#"{{"type": "ListRelationship", "objectList": ["urn:ngsi-ld:Room:{namespace}-1", "urn:ngsi-ld:Room:{namespace}-2"]}}"#).parse()?;
    let document = UncheckedDocument::from_entity(&building_value_list_null, Lossless::Normalized).set_member("hasRooms", &has_rooms);
    let request = broker
        .unchecked(&CreateEntity::builder().entity(&building_value_list_null).build())?
        .document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    let deletion = Change::Delete(InstanceDeletion::new(AttributeKind::ListProperty));
    broker
        .entities()
        .update_attribute(&building_value_list_null_id, &"tags".parse::<AttributeName>()?, deletion)
        .send()
        .await?
        .done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_value_list_null_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, lacks_attribute("tags"));
    expect_that!(entity, has_attribute("hasRooms", any_attribute()));
    Ok(())
}
