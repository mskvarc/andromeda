//! A create refuses the null beside data.

use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    problem::{answered, problem},
    value::object,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{
        UncheckedDocument,
        headers::Accept,
        operation::{CreateEntity, CreateOrUpdateTemporalEvolution},
    },
    model::{Entity, EntityId, ErrorType, ListProperty, Lossless, NgsiLdMediaType, RawJson, Value, evolution::TemporalEntity},
};

/// Pins: Create Entity and Create or Update Temporal Evolution each answer 400 `BadRequestData`, and store nothing,
/// for urn:ngsi-ld:null beside data in a Property array value, a Relationship object array, a `valueList`, an
/// `objectList` or an array within a Property value
/// Clause: 5.5.4, 4.5.0, 5.2.1, 5.6.11.4
#[gtest]
#[tokio::test]
async fn n_ent_cre_9021_a_create_refuses_the_null_beside_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_beside_data_tags_id = namespace.urn("Building", "beside-data-tags")?;
    let building_type = namespace.entity_type("Building")?;
    let building_beside_data_owners_id = namespace.urn("Building", "beside-data-owners")?;
    let person_a_id = namespace.urn("Person", "a")?;
    let building_beside_data_labels_id = namespace.urn("Building", "beside-data-labels")?;
    let building_beside_data_rooms_id = namespace.urn("Building", "beside-data-rooms")?;
    let building_beside_data_address_id = namespace.urn("Building", "beside-data-address")?;
    let building_beside_data_tags = Entity::builder()
        .id(building_beside_data_tags_id.clone())
        .entity_type(building_type.clone())
        .property("tags", Value::Array(vec![Value::from("a"), Value::from("urn:ngsi-ld:null")]))
        .build()?;
    let answer = broker.with_responses().entities().create(&building_beside_data_tags).send().await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let building_beside_data_tags_evolution = TemporalEntity::builder()
        .id(building_beside_data_tags_id.clone())
        .entity_type(building_type.clone())
        .property("tags", Value::Array(vec![Value::from("a"), Value::from("urn:ngsi-ld:null")]))
        .build()?;
    let answer_2 = broker
        .with_responses()
        .temporal()
        .create_or_update(&building_beside_data_tags_evolution)
        .send()
        .await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_3 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_beside_data_tags_id)
        .send()
        .await;
    expect_that!(&answer_3, answered(StatusCode::NOT_FOUND));
    let building_beside_data_owners = Entity::builder()
        .id(building_beside_data_owners_id.clone())
        .entity_type(building_type.clone())
        .relationship("owners", vec![person_a_id.clone(), "urn:ngsi-ld:null".parse::<EntityId>()?])
        .build()?;
    let answer_4 = broker.with_responses().entities().create(&building_beside_data_owners).send().await;
    expect_that!(&answer_4, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let building_beside_data_owners_evolution = TemporalEntity::builder()
        .id(building_beside_data_owners_id.clone())
        .entity_type(building_type.clone())
        .relationship("owners", vec![person_a_id.clone(), "urn:ngsi-ld:null".parse::<EntityId>()?])
        .build()?;
    let answer_5 = broker
        .with_responses()
        .temporal()
        .create_or_update(&building_beside_data_owners_evolution)
        .send()
        .await;
    expect_that!(&answer_5, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_6 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_beside_data_owners_id)
        .send()
        .await;
    expect_that!(&answer_6, answered(StatusCode::NOT_FOUND));
    let building_beside_data_labels = Entity::builder()
        .id(building_beside_data_labels_id.clone())
        .entity_type(building_type.clone())
        .list_property("labels", ListProperty::builder(vec![Value::from("a"), Value::from("urn:ngsi-ld:null")]))
        .build()?;
    let answer_7 = broker.with_responses().entities().create(&building_beside_data_labels).send().await;
    expect_that!(&answer_7, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let building_beside_data_labels_evolution = TemporalEntity::builder()
        .id(building_beside_data_labels_id.clone())
        .entity_type(building_type.clone())
        .list_property("labels", ListProperty::builder(vec![Value::from("a"), Value::from("urn:ngsi-ld:null")]))
        .build()?;
    let answer_8 = broker
        .with_responses()
        .temporal()
        .create_or_update(&building_beside_data_labels_evolution)
        .send()
        .await;
    expect_that!(&answer_8, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_9 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_beside_data_labels_id)
        .send()
        .await;
    expect_that!(&answer_9, answered(StatusCode::NOT_FOUND));
    let building_beside_data_rooms = Entity::builder()
        .id(building_beside_data_rooms_id.clone())
        .entity_type(building_type.clone())
        .build()?;
    // The model refuses it (the model reads it as written otherwise [at $.rooms.objectList[0] (written "urn:ngsi-ld:Room:zqns-a")]: {"id":"urn:ngsi-ld:Building:zqns-beside-data-rooms","type":"Building_zqns","rooms":{"type":"ListRelationship","objectList":[{"object":"urn:ngsi-ld:Room:zqns-a"},{"object":"urn:ngsi-ld:null"}]}}), so `rooms` is written as text.
    let rooms: RawJson = format!(r#"{{"type": "ListRelationship", "objectList": ["urn:ngsi-ld:Room:{namespace}-a", "urn:ngsi-ld:null"]}}"#).parse()?;
    let document = UncheckedDocument::from_entity(&building_beside_data_rooms, Lossless::Normalized).set_member("rooms", &rooms);
    let request = broker
        .unchecked(&CreateEntity::builder().entity(&building_beside_data_rooms).build())?
        .document(&document);
    let answer_10 = broker.send_unchecked(request).await;
    expect_that!(&answer_10, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let building_beside_data_rooms_evolution = TemporalEntity::builder()
        .id(building_beside_data_rooms_id.clone())
        .entity_type(building_type.clone())
        .build()?;
    // The model refuses it (the model reads it as written otherwise [at $.rooms[0].objectList[0] (written "urn:ngsi-ld:Room:zqns-a")]: {"id":"urn:ngsi-ld:Building:zqns-beside-data-rooms","type":"Building_zqns","rooms":[{"type":"ListRelationship","objectList":[{"object":"urn:ngsi-ld:Room:zqns-a"},{"object":"urn:ngsi-ld:null"}]}]}), so `rooms` is written as text.
    let rooms_2: RawJson = format!(r#"[{{"type": "ListRelationship", "objectList": ["urn:ngsi-ld:Room:{namespace}-a", "urn:ngsi-ld:null"]}}]"#).parse()?;
    let document_2 = UncheckedDocument::try_from(building_beside_data_rooms_evolution.render(Lossless::Normalized))?.set_member("rooms", &rooms_2);
    let request_2 = broker
        .unchecked(&CreateOrUpdateTemporalEvolution::builder().entity(&building_beside_data_rooms_evolution).build())?
        .document(&document_2);
    let answer_11 = broker.send_unchecked(request_2).await;
    expect_that!(&answer_11, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_12 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_beside_data_rooms_id)
        .send()
        .await;
    expect_that!(&answer_12, answered(StatusCode::NOT_FOUND));
    let building_beside_data_address = Entity::builder()
        .id(building_beside_data_address_id.clone())
        .entity_type(building_type.clone())
        .property(
            "address",
            object([("lines", Value::Array(vec![Value::from("Pariser Platz"), Value::from("urn:ngsi-ld:null")]))]),
        )
        .build()?;
    let answer_13 = broker.with_responses().entities().create(&building_beside_data_address).send().await;
    expect_that!(&answer_13, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let building_beside_data_address_evolution = TemporalEntity::builder()
        .id(building_beside_data_address_id.clone())
        .entity_type(building_type.clone())
        .property(
            "address",
            object([("lines", Value::Array(vec![Value::from("Pariser Platz"), Value::from("urn:ngsi-ld:null")]))]),
        )
        .build()?;
    let answer_14 = broker
        .with_responses()
        .temporal()
        .create_or_update(&building_beside_data_address_evolution)
        .send()
        .await;
    expect_that!(&answer_14, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_15 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_beside_data_address_id)
        .send()
        .await;
    expect_that!(&answer_15, answered(StatusCode::NOT_FOUND));
    Ok(())
}
