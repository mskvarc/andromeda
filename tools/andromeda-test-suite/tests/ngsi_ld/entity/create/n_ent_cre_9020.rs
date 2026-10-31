//! A create refuses every family deletion spelling.

use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    problem::{answered, problem},
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::CreateEntity},
    model::{Entity, ErrorType, Lossless, NgsiLdMediaType, RawJson},
};

/// Pins: Create Entity answers 400 `BadRequestData` and stores nothing for a `LanguageProperty` `languageMap`
/// {"@none": null}, a `ListProperty` `valueList` or `ListRelationship` `objectList` of one null, in either
/// `objectList` form, and a Relationship object of urn:ngsi-ld:null
/// Clause: 5.5.4, 4.5.18, 4.5.21.2, 4.5.22.2, 5.2.1
#[gtest]
#[tokio::test]
async fn n_ent_cre_9020_a_create_refuses_every_family_deletion_spelling() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_create_language_map_id = namespace.urn("Building", "create-language-map")?;
    let building_type = namespace.entity_type("Building")?;
    let building_create_value_list_id = namespace.urn("Building", "create-value-list")?;
    let building_create_object_list_id = namespace.urn("Building", "create-object-list")?;
    let building_create_object_list_normalized_id = namespace.urn("Building", "create-object-list-normalized")?;
    let building_create_relationship_object_id = namespace.urn("Building", "create-relationship-object")?;
    let building_create_language_map = Entity::builder()
        .id(building_create_language_map_id.clone())
        .entity_type(building_type.clone())
        .build()?;
    // The model refuses it (a request cannot hold the NGSI-LD Null in `languageMap` at label), so `label` is written as text.
    let label: RawJson = r#"{"type": "LanguageProperty", "languageMap": {"@none": "urn:ngsi-ld:null"}}"#.parse()?;
    let document = UncheckedDocument::from_entity(&building_create_language_map, Lossless::Normalized).set_member("label", &label);
    let request = broker
        .unchecked(&CreateEntity::builder().entity(&building_create_language_map).build())?
        .document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_create_language_map_id)
        .send()
        .await;
    expect_that!(&answer_2, answered(StatusCode::NOT_FOUND));
    let building_create_value_list = Entity::builder()
        .id(building_create_value_list_id.clone())
        .entity_type(building_type.clone())
        .build()?;
    // The model refuses it (a request cannot hold the NGSI-LD Null in `valueList` at tags), so `tags` is written as text.
    let tags: RawJson = r#"{"type": "ListProperty", "valueList": ["urn:ngsi-ld:null"]}"#.parse()?;
    let document_2 = UncheckedDocument::from_entity(&building_create_value_list, Lossless::Normalized).set_member("tags", &tags);
    let request_2 = broker
        .unchecked(&CreateEntity::builder().entity(&building_create_value_list).build())?
        .document(&document_2);
    let answer_3 = broker.send_unchecked(request_2).await;
    expect_that!(&answer_3, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_4 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_create_value_list_id)
        .send()
        .await;
    expect_that!(&answer_4, answered(StatusCode::NOT_FOUND));
    let building_create_object_list = Entity::builder()
        .id(building_create_object_list_id.clone())
        .entity_type(building_type.clone())
        .build()?;
    // The model refuses it (a request cannot hold the NGSI-LD Null in `objectList` at hasRooms), so `hasRooms` is written as text.
    let has_rooms: RawJson = r#"{"type": "ListRelationship", "objectList": ["urn:ngsi-ld:null"]}"#.parse()?;
    let document_3 = UncheckedDocument::from_entity(&building_create_object_list, Lossless::Normalized).set_member("hasRooms", &has_rooms);
    let request_3 = broker
        .unchecked(&CreateEntity::builder().entity(&building_create_object_list).build())?
        .document(&document_3);
    let answer_5 = broker.send_unchecked(request_3).await;
    expect_that!(&answer_5, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_6 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_create_object_list_id)
        .send()
        .await;
    expect_that!(&answer_6, answered(StatusCode::NOT_FOUND));
    let building_create_object_list_normalized = Entity::builder()
        .id(building_create_object_list_normalized_id.clone())
        .entity_type(building_type.clone())
        .build()?;
    // The model refuses it (a request cannot hold the NGSI-LD Null in `objectList` at hasRooms), so `hasRooms` is written as text.
    let has_rooms_2: RawJson = r#"{"type": "ListRelationship", "objectList": [{"object": "urn:ngsi-ld:null"}]}"#.parse()?;
    let document_4 = UncheckedDocument::from_entity(&building_create_object_list_normalized, Lossless::Normalized).set_member("hasRooms", &has_rooms_2);
    let request_4 = broker
        .unchecked(&CreateEntity::builder().entity(&building_create_object_list_normalized).build())?
        .document(&document_4);
    let answer_7 = broker.send_unchecked(request_4).await;
    expect_that!(&answer_7, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_8 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_create_object_list_normalized_id)
        .send()
        .await;
    expect_that!(&answer_8, answered(StatusCode::NOT_FOUND));
    let building_create_relationship_object = Entity::builder()
        .id(building_create_relationship_object_id.clone())
        .entity_type(building_type.clone())
        .build()?;
    // The model refuses it (a request cannot hold the NGSI-LD Null in `object` at belongsTo), so `belongsTo` is written as text.
    let belongs_to: RawJson = r#"{"type": "Relationship", "object": "urn:ngsi-ld:null"}"#.parse()?;
    let document_5 = UncheckedDocument::from_entity(&building_create_relationship_object, Lossless::Normalized).set_member("belongsTo", &belongs_to);
    let request_5 = broker
        .unchecked(&CreateEntity::builder().entity(&building_create_relationship_object).build())?
        .document(&document_5);
    let answer_9 = broker.send_unchecked(request_5).await;
    expect_that!(&answer_9, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_10 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_create_relationship_object_id)
        .send()
        .await;
    expect_that!(&answer_10, answered(StatusCode::NOT_FOUND));
    Ok(())
}
