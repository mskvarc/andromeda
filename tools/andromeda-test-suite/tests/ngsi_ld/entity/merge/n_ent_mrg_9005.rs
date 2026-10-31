//! Merge and update refuse the null beside data.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::{has_attribute, lacks_attribute},
    namespace::Namespace,
    outcome::Done,
    problem::problem,
    value::object,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{
        UncheckedDocument,
        headers::Accept,
        operation::{MergeEntity, UpdateAttributes},
    },
    model::{Entity, EntityFragment, ErrorType, Lossless, NgsiLdMediaType, RawJson, Value},
};

/// Pins: Merge Entity and Update Attributes each answer 400 `BadRequestData` for urn:ngsi-ld:null beside data in
/// any of five attribute shapes, and the Entity stays readable and unchanged
/// Clause: 5.5.4, 5.5.12, 5.5.8, 4.5.0
#[gtest]
#[tokio::test]
async fn n_ent_mrg_9005_merge_and_update_refuse_the_null_beside_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_beside_data_merge_id = namespace.urn("Building", "beside-data-merge")?;
    let building_type = namespace.entity_type("Building")?;
    let building_beside_data_merge = Entity::builder()
        .id(building_beside_data_merge_id.clone())
        .entity_type(building_type.clone())
        .property(
            "address",
            object([
                ("street", Value::from("Strasse des 17. Juni")),
                ("city", Value::from("Berlin")),
                ("country", Value::from("Germany")),
            ]),
        )
        .build()?;
    broker.entities().create(&building_beside_data_merge).send().await?.done()?;
    let fragment = EntityFragment::for_entity(building_beside_data_merge_id.clone(), building_type.clone()).build()?;
    // The model refuses it (an attribute patch deleting members: {"type":"Property","value":["a","urn:ngsi-ld:null"]}), so `tags` is written as text.
    let tags: RawJson = r#"{"type": "Property", "value": ["a", "urn:ngsi-ld:null"]}"#.parse()?;
    let document = UncheckedDocument::try_from(fragment.render(Lossless::Normalized))?.set_member("tags", &tags);
    let request = broker
        .unchecked(&MergeEntity::builder().id(&building_beside_data_merge_id).fragment(&fragment).build())?
        .document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let fragment_2 = EntityFragment::builder().build()?;
    // The model refuses it (an attribute patch deleting members: {"type":"Property","value":["a","urn:ngsi-ld:null"]}), so `tags` is written as text.
    let tags_2: RawJson = r#"{"type": "Property", "value": ["a", "urn:ngsi-ld:null"]}"#.parse()?;
    let document_2 = UncheckedDocument::try_from(fragment_2.render(Lossless::Normalized))?.set_member("tags", &tags_2);
    let request_2 = broker
        .unchecked(&UpdateAttributes::builder().id(&building_beside_data_merge_id).fragment(&fragment_2).build())?
        .document(&document_2);
    let answer_2 = broker.send_unchecked(request_2).await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let fragment_3 = EntityFragment::for_entity(building_beside_data_merge_id.clone(), building_type.clone()).build()?;
    // The model refuses it (an attribute patch deleting members: {"type":"Relationship","object":["urn:ngsi-ld:Person:zqns-a","urn:ngsi-ld:null"]}), so `owners` is written as text.
    let owners: RawJson = format!(r#"{{"type": "Relationship", "object": ["urn:ngsi-ld:Person:{namespace}-a", "urn:ngsi-ld:null"]}}"#).parse()?;
    let document_3 = UncheckedDocument::try_from(fragment_3.render(Lossless::Normalized))?.set_member("owners", &owners);
    let request_3 = broker
        .unchecked(&MergeEntity::builder().id(&building_beside_data_merge_id).fragment(&fragment_3).build())?
        .document(&document_3);
    let answer_3 = broker.send_unchecked(request_3).await;
    expect_that!(&answer_3, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let fragment_4 = EntityFragment::builder().build()?;
    // The model refuses it (an attribute patch deleting members: {"type":"Relationship","object":["urn:ngsi-ld:Person:zqns-a","urn:ngsi-ld:null"]}), so `owners` is written as text.
    let owners_2: RawJson = format!(r#"{{"type": "Relationship", "object": ["urn:ngsi-ld:Person:{namespace}-a", "urn:ngsi-ld:null"]}}"#).parse()?;
    let document_4 = UncheckedDocument::try_from(fragment_4.render(Lossless::Normalized))?.set_member("owners", &owners_2);
    let request_4 = broker
        .unchecked(&UpdateAttributes::builder().id(&building_beside_data_merge_id).fragment(&fragment_4).build())?
        .document(&document_4);
    let answer_4 = broker.send_unchecked(request_4).await;
    expect_that!(&answer_4, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let fragment_5 = EntityFragment::for_entity(building_beside_data_merge_id.clone(), building_type.clone()).build()?;
    // The model refuses it (an attribute patch deleting members: {"type":"ListProperty","valueList":["a","urn:ngsi-ld:null"]}), so `labels` is written as text.
    let labels: RawJson = r#"{"type": "ListProperty", "valueList": ["a", "urn:ngsi-ld:null"]}"#.parse()?;
    let document_5 = UncheckedDocument::try_from(fragment_5.render(Lossless::Normalized))?.set_member("labels", &labels);
    let request_5 = broker
        .unchecked(&MergeEntity::builder().id(&building_beside_data_merge_id).fragment(&fragment_5).build())?
        .document(&document_5);
    let answer_5 = broker.send_unchecked(request_5).await;
    expect_that!(&answer_5, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let fragment_6 = EntityFragment::builder().build()?;
    // The model refuses it (an attribute patch deleting members: {"type":"ListProperty","valueList":["a","urn:ngsi-ld:null"]}), so `labels` is written as text.
    let labels_2: RawJson = r#"{"type": "ListProperty", "valueList": ["a", "urn:ngsi-ld:null"]}"#.parse()?;
    let document_6 = UncheckedDocument::try_from(fragment_6.render(Lossless::Normalized))?.set_member("labels", &labels_2);
    let request_6 = broker
        .unchecked(&UpdateAttributes::builder().id(&building_beside_data_merge_id).fragment(&fragment_6).build())?
        .document(&document_6);
    let answer_6 = broker.send_unchecked(request_6).await;
    expect_that!(&answer_6, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let fragment_7 = EntityFragment::for_entity(building_beside_data_merge_id.clone(), building_type.clone()).build()?;
    // The model refuses it (the model reads it as written otherwise [at $.rooms.objectList[0] (written "urn:ngsi-ld:Room:zqns-a")]: {"id":"urn:ngsi-ld:Building:zqns-beside-data-merge","type":"Building_zqns","rooms":{"type":"ListRelationship","objectList":[{"object":"urn:ngsi-ld:Room:zqns-a"},{"object":"urn:ngsi-ld:null"}]}}), so `rooms` is written as text.
    let rooms: RawJson = format!(r#"{{"type": "ListRelationship", "objectList": ["urn:ngsi-ld:Room:{namespace}-a", "urn:ngsi-ld:null"]}}"#).parse()?;
    let document_7 = UncheckedDocument::try_from(fragment_7.render(Lossless::Normalized))?.set_member("rooms", &rooms);
    let request_7 = broker
        .unchecked(&MergeEntity::builder().id(&building_beside_data_merge_id).fragment(&fragment_7).build())?
        .document(&document_7);
    let answer_7 = broker.send_unchecked(request_7).await;
    expect_that!(&answer_7, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let fragment_8 = EntityFragment::builder().build()?;
    // The model refuses it (the model reads it as written otherwise [at $.rooms.objectList[0] (written "urn:ngsi-ld:Room:zqns-a")]: {"rooms":{"type":"ListRelationship","objectList":[{"object":"urn:ngsi-ld:Room:zqns-a"},{"object":"urn:ngsi-ld:null"}]}}), so `rooms` is written as text.
    let rooms_2: RawJson = format!(r#"{{"type": "ListRelationship", "objectList": ["urn:ngsi-ld:Room:{namespace}-a", "urn:ngsi-ld:null"]}}"#).parse()?;
    let document_8 = UncheckedDocument::try_from(fragment_8.render(Lossless::Normalized))?.set_member("rooms", &rooms_2);
    let request_8 = broker
        .unchecked(&UpdateAttributes::builder().id(&building_beside_data_merge_id).fragment(&fragment_8).build())?
        .document(&document_8);
    let answer_8 = broker.send_unchecked(request_8).await;
    expect_that!(&answer_8, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let fragment_9 = EntityFragment::for_entity(building_beside_data_merge_id.clone(), building_type.clone()).build()?;
    // The model refuses it (an attribute patch deleting members: {"type":"Property","value":{"lines":["Pariser Platz","urn:ngsi-ld:null"]}}), so `address` is written as text.
    let address: RawJson = r#"{"type": "Property", "value": {"lines": ["Pariser Platz", "urn:ngsi-ld:null"]}}"#.parse()?;
    let document_9 = UncheckedDocument::try_from(fragment_9.render(Lossless::Normalized))?.set_member("address", &address);
    let request_9 = broker
        .unchecked(&MergeEntity::builder().id(&building_beside_data_merge_id).fragment(&fragment_9).build())?
        .document(&document_9);
    let answer_9 = broker.send_unchecked(request_9).await;
    expect_that!(&answer_9, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let fragment_10 = EntityFragment::builder().build()?;
    // The model refuses it (an attribute patch deleting members: {"type":"Property","value":{"lines":["Pariser Platz","urn:ngsi-ld:null"]}}), so `address` is written as text.
    let address_2: RawJson = r#"{"type": "Property", "value": {"lines": ["Pariser Platz", "urn:ngsi-ld:null"]}}"#.parse()?;
    let document_10 = UncheckedDocument::try_from(fragment_10.render(Lossless::Normalized))?.set_member("address", &address_2);
    let request_10 = broker
        .unchecked(&UpdateAttributes::builder().id(&building_beside_data_merge_id).fragment(&fragment_10).build())?
        .document(&document_10);
    let answer_10 = broker.send_unchecked(request_10).await;
    expect_that!(&answer_10, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_beside_data_merge_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, lacks_attribute("tags"));
    expect_that!(entity, lacks_attribute("owners"));
    expect_that!(entity, lacks_attribute("labels"));
    expect_that!(entity, lacks_attribute("rooms"));
    expect_that!(
        entity,
        has_attribute(
            "address",
            any_attribute()
                .value_member(["street"], "Strasse des 17. Juni")
                .value_member(["city"], "Berlin")
                .value_member(["country"], "Germany")
                .without_value_member(["lines"])
        )
    );
    Ok(())
}
