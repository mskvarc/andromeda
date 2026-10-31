//! Every unitless family refuses a unit.

use andromeda_test_suite::{broker, namespace::Namespace, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, operation::CreateEntity},
    model::{Entity, ErrorType, Lossless, RawJson},
};

/// Pins: Create Entity with a `unitCode` on a Relationship, a `LanguageProperty`, a `VocabProperty`, a
/// `JsonProperty` and a `ListRelationship` answers 400 `BadRequestData` each
/// Clause: 4.5.3.2, 4.5.18.2, 4.5.20.2, 4.5.22.2, 4.5.24.2
#[gtest]
#[tokio::test]
async fn n_cmn_uni_9002_every_unitless_family_refuses_a_unit() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_unitless_relationship_id = namespace.urn("Building", "unitless-Relationship")?;
    let building_type = namespace.entity_type("Building")?;
    let building_unitless_language_property_id = namespace.urn("Building", "unitless-LanguageProperty")?;
    let building_unitless_vocab_property_id = namespace.urn("Building", "unitless-VocabProperty")?;
    let building_unitless_json_property_id = namespace.urn("Building", "unitless-JsonProperty")?;
    let building_unitless_list_relationship_id = namespace.urn("Building", "unitless-ListRelationship")?;
    let building_unitless_relationship = Entity::builder()
        .id(building_unitless_relationship_id)
        .entity_type(building_type.clone())
        .build()?;
    // The model refuses it (member `unitCode` is not allowed here at measure), so `measure` is written as text.
    let measure: RawJson = format!(r#"{{"type": "Relationship", "object": "urn:ngsi-ld:Room:{namespace}-1", "unitCode": "MMT"}}"#).parse()?;
    let document = UncheckedDocument::from_entity(&building_unitless_relationship, Lossless::Normalized).set_member("measure", &measure);
    let request = broker
        .unchecked(&CreateEntity::builder().entity(&building_unitless_relationship).build())?
        .document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let building_unitless_language_property = Entity::builder()
        .id(building_unitless_language_property_id)
        .entity_type(building_type.clone())
        .build()?;
    // The model refuses it (member `unitCode` is not allowed here at measure), so `measure` is written as text.
    let measure_2: RawJson = r#"{"type": "LanguageProperty", "languageMap": {"en": "Great Market"}, "unitCode": "MMT"}"#.parse()?;
    let document_2 = UncheckedDocument::from_entity(&building_unitless_language_property, Lossless::Normalized).set_member("measure", &measure_2);
    let request_2 = broker
        .unchecked(&CreateEntity::builder().entity(&building_unitless_language_property).build())?
        .document(&document_2);
    let answer_2 = broker.send_unchecked(request_2).await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let building_unitless_vocab_property = Entity::builder()
        .id(building_unitless_vocab_property_id)
        .entity_type(building_type.clone())
        .build()?;
    // The model refuses it (member `unitCode` is not allowed here at measure), so `measure` is written as text.
    let measure_3: RawJson = r#"{"type": "VocabProperty", "vocab": "commercial", "unitCode": "MMT"}"#.parse()?;
    let document_3 = UncheckedDocument::from_entity(&building_unitless_vocab_property, Lossless::Normalized).set_member("measure", &measure_3);
    let request_3 = broker
        .unchecked(&CreateEntity::builder().entity(&building_unitless_vocab_property).build())?
        .document(&document_3);
    let answer_3 = broker.send_unchecked(request_3).await;
    expect_that!(&answer_3, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let building_unitless_json_property = Entity::builder()
        .id(building_unitless_json_property_id)
        .entity_type(building_type.clone())
        .build()?;
    // The model refuses it (member `unitCode` is not allowed here at measure), so `measure` is written as text.
    let measure_4: RawJson = r#"{"type": "JsonProperty", "json": {"a": 1}, "unitCode": "MMT"}"#.parse()?;
    let document_4 = UncheckedDocument::from_entity(&building_unitless_json_property, Lossless::Normalized).set_member("measure", &measure_4);
    let request_4 = broker
        .unchecked(&CreateEntity::builder().entity(&building_unitless_json_property).build())?
        .document(&document_4);
    let answer_4 = broker.send_unchecked(request_4).await;
    expect_that!(&answer_4, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let building_unitless_list_relationship = Entity::builder()
        .id(building_unitless_list_relationship_id)
        .entity_type(building_type.clone())
        .build()?;
    // The model refuses it (member `unitCode` is not allowed here at measure), so `measure` is written as text.
    let measure_5: RawJson = format!(r#"{{"type": "ListRelationship", "objectList": ["urn:ngsi-ld:Room:{namespace}-1"], "unitCode": "MMT"}}"#).parse()?;
    let document_5 = UncheckedDocument::from_entity(&building_unitless_list_relationship, Lossless::Normalized).set_member("measure", &measure_5);
    let request_5 = broker
        .unchecked(&CreateEntity::builder().entity(&building_unitless_list_relationship).build())?
        .document(&document_5);
    let answer_5 = broker.send_unchecked(request_5).await;
    expect_that!(&answer_5, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    Ok(())
}
