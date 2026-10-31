//! A seal outside ngsildproof is bad request data on every family.

use andromeda_test_suite::{broker, namespace::Namespace, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, operation::CreateEntity},
    model::{Entity, ErrorType, Lossless, RawJson},
};

/// Pins: Create Entity with `entityIdSealed` or `entityTypeSealed` on an attribute of each of the eight families
/// not named ngsildproof answers 400 `BadRequestData`
/// Clause: 4.5.2.2, 4.5.3.2, 5.6.1.4
#[gtest]
#[tokio::test]
async fn n_ent_cre_9022_a_seal_outside_ngsildproof_is_bad_request_data_on_every_family() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_forged_property_entity_id_sealed_id = namespace.urn("Building", "forged-Property-entityIdSealed")?;
    let building_type = namespace.entity_type("Building")?;
    let building_forged_property_entity_type_sealed_id = namespace.urn("Building", "forged-Property-entityTypeSealed")?;
    let building_forged_relationship_entity_id_sealed_id = namespace.urn("Building", "forged-Relationship-entityIdSealed")?;
    let building_forged_relationship_entity_type_seal_id = namespace.urn("Building", "forged-Relationship-entityTypeSealed")?;
    let building_forged_geo_property_entity_id_sealed_id = namespace.urn("Building", "forged-GeoProperty-entityIdSealed")?;
    let building_forged_geo_property_entity_type_seal_id = namespace.urn("Building", "forged-GeoProperty-entityTypeSealed")?;
    let building_forged_language_property_entity_id_s_id = namespace.urn("Building", "forged-LanguageProperty-entityIdSealed")?;
    let building_forged_language_property_entity_type_id = namespace.urn("Building", "forged-LanguageProperty-entityTypeSealed")?;
    let building_forged_vocab_property_entity_id_seal_id = namespace.urn("Building", "forged-VocabProperty-entityIdSealed")?;
    let building_forged_vocab_property_entity_type_se_id = namespace.urn("Building", "forged-VocabProperty-entityTypeSealed")?;
    let building_forged_list_property_entity_id_seale_id = namespace.urn("Building", "forged-ListProperty-entityIdSealed")?;
    let building_forged_list_property_entity_type_sea_id = namespace.urn("Building", "forged-ListProperty-entityTypeSealed")?;
    let building_forged_list_relationship_entity_id_s_id = namespace.urn("Building", "forged-ListRelationship-entityIdSealed")?;
    let building_forged_list_relationship_entity_type_id = namespace.urn("Building", "forged-ListRelationship-entityTypeSealed")?;
    let building_forged_json_property_entity_id_seale_id = namespace.urn("Building", "forged-JsonProperty-entityIdSealed")?;
    let building_forged_json_property_entity_type_sea_id = namespace.urn("Building", "forged-JsonProperty-entityTypeSealed")?;
    let building_forged_property_entity_id_sealed = Entity::builder()
        .id(building_forged_property_entity_id_sealed_id)
        .entity_type(building_type.clone())
        .build()?;
    // The model refuses it (member `entityIdSealed` is not allowed here at sealed), so `sealed` is written as text.
    let sealed: RawJson = format!(r#"{{"type": "Property", "value": 21.5, "entityIdSealed": "Building_{namespace}"}}"#).parse()?;
    let document = UncheckedDocument::from_entity(&building_forged_property_entity_id_sealed, Lossless::Normalized).set_member("sealed", &sealed);
    let request = broker
        .unchecked(&CreateEntity::builder().entity(&building_forged_property_entity_id_sealed).build())?
        .document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let building_forged_property_entity_type_sealed = Entity::builder()
        .id(building_forged_property_entity_type_sealed_id)
        .entity_type(building_type.clone())
        .build()?;
    // The model refuses it (member `entityTypeSealed` is not allowed here at sealed), so `sealed` is written as text.
    let sealed_2: RawJson = format!(r#"{{"type": "Property", "value": 21.5, "entityTypeSealed": "Building_{namespace}"}}"#).parse()?;
    let document_2 = UncheckedDocument::from_entity(&building_forged_property_entity_type_sealed, Lossless::Normalized).set_member("sealed", &sealed_2);
    let request_2 = broker
        .unchecked(&CreateEntity::builder().entity(&building_forged_property_entity_type_sealed).build())?
        .document(&document_2);
    let answer_2 = broker.send_unchecked(request_2).await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let building_forged_relationship_entity_id_sealed = Entity::builder()
        .id(building_forged_relationship_entity_id_sealed_id)
        .entity_type(building_type.clone())
        .build()?;
    // The model refuses it (member `entityIdSealed` is not allowed here at sealed), so `sealed` is written as text.
    let sealed_3: RawJson =
        format!(r#"{{"type": "Relationship", "object": "urn:ngsi-ld:Site:{namespace}-1", "entityIdSealed": "Building_{namespace}"}}"#).parse()?;
    let document_3 = UncheckedDocument::from_entity(&building_forged_relationship_entity_id_sealed, Lossless::Normalized).set_member("sealed", &sealed_3);
    let request_3 = broker
        .unchecked(&CreateEntity::builder().entity(&building_forged_relationship_entity_id_sealed).build())?
        .document(&document_3);
    let answer_3 = broker.send_unchecked(request_3).await;
    expect_that!(&answer_3, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let building_forged_relationship_entity_type_seal = Entity::builder()
        .id(building_forged_relationship_entity_type_seal_id)
        .entity_type(building_type.clone())
        .build()?;
    // The model refuses it (member `entityTypeSealed` is not allowed here at sealed), so `sealed` is written as text.
    let sealed_4: RawJson =
        format!(r#"{{"type": "Relationship", "object": "urn:ngsi-ld:Site:{namespace}-1", "entityTypeSealed": "Building_{namespace}"}}"#).parse()?;
    let document_4 = UncheckedDocument::from_entity(&building_forged_relationship_entity_type_seal, Lossless::Normalized).set_member("sealed", &sealed_4);
    let request_4 = broker
        .unchecked(&CreateEntity::builder().entity(&building_forged_relationship_entity_type_seal).build())?
        .document(&document_4);
    let answer_4 = broker.send_unchecked(request_4).await;
    expect_that!(&answer_4, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let building_forged_geo_property_entity_id_sealed = Entity::builder()
        .id(building_forged_geo_property_entity_id_sealed_id)
        .entity_type(building_type.clone())
        .build()?;
    // The model refuses it (member `entityIdSealed` is not allowed here at sealed), so `sealed` is written as text.
    let sealed_5: RawJson =
        format!(r#"{{"type": "GeoProperty", "value": {{"type": "Point", "coordinates": [13.4, 52.5]}}, "entityIdSealed": "Building_{namespace}"}}"#).parse()?;
    let document_5 = UncheckedDocument::from_entity(&building_forged_geo_property_entity_id_sealed, Lossless::Normalized).set_member("sealed", &sealed_5);
    let request_5 = broker
        .unchecked(&CreateEntity::builder().entity(&building_forged_geo_property_entity_id_sealed).build())?
        .document(&document_5);
    let answer_5 = broker.send_unchecked(request_5).await;
    expect_that!(&answer_5, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let building_forged_geo_property_entity_type_seal = Entity::builder()
        .id(building_forged_geo_property_entity_type_seal_id)
        .entity_type(building_type.clone())
        .build()?;
    // The model refuses it (member `entityTypeSealed` is not allowed here at sealed), so `sealed` is written as text.
    let sealed_6: RawJson =
        format!(r#"{{"type": "GeoProperty", "value": {{"type": "Point", "coordinates": [13.4, 52.5]}}, "entityTypeSealed": "Building_{namespace}"}}"#)
            .parse()?;
    let document_6 = UncheckedDocument::from_entity(&building_forged_geo_property_entity_type_seal, Lossless::Normalized).set_member("sealed", &sealed_6);
    let request_6 = broker
        .unchecked(&CreateEntity::builder().entity(&building_forged_geo_property_entity_type_seal).build())?
        .document(&document_6);
    let answer_6 = broker.send_unchecked(request_6).await;
    expect_that!(&answer_6, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let building_forged_language_property_entity_id_s = Entity::builder()
        .id(building_forged_language_property_entity_id_s_id)
        .entity_type(building_type.clone())
        .build()?;
    // The model refuses it (member `entityIdSealed` is not allowed here at sealed), so `sealed` is written as text.
    let sealed_7: RawJson =
        format!(r#"{{"type": "LanguageProperty", "languageMap": {{"en": "Door", "de": "Tuer"}}, "entityIdSealed": "Building_{namespace}"}}"#).parse()?;
    let document_7 = UncheckedDocument::from_entity(&building_forged_language_property_entity_id_s, Lossless::Normalized).set_member("sealed", &sealed_7);
    let request_7 = broker
        .unchecked(&CreateEntity::builder().entity(&building_forged_language_property_entity_id_s).build())?
        .document(&document_7);
    let answer_7 = broker.send_unchecked(request_7).await;
    expect_that!(&answer_7, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let building_forged_language_property_entity_type = Entity::builder()
        .id(building_forged_language_property_entity_type_id)
        .entity_type(building_type.clone())
        .build()?;
    // The model refuses it (member `entityTypeSealed` is not allowed here at sealed), so `sealed` is written as text.
    let sealed_8: RawJson =
        format!(r#"{{"type": "LanguageProperty", "languageMap": {{"en": "Door", "de": "Tuer"}}, "entityTypeSealed": "Building_{namespace}"}}"#).parse()?;
    let document_8 = UncheckedDocument::from_entity(&building_forged_language_property_entity_type, Lossless::Normalized).set_member("sealed", &sealed_8);
    let request_8 = broker
        .unchecked(&CreateEntity::builder().entity(&building_forged_language_property_entity_type).build())?
        .document(&document_8);
    let answer_8 = broker.send_unchecked(request_8).await;
    expect_that!(&answer_8, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let building_forged_vocab_property_entity_id_seal = Entity::builder()
        .id(building_forged_vocab_property_entity_id_seal_id)
        .entity_type(building_type.clone())
        .build()?;
    // The model refuses it (member `entityIdSealed` is not allowed here at sealed), so `sealed` is written as text.
    let sealed_9: RawJson = format!(r#"{{"type": "VocabProperty", "vocab": "sensor", "entityIdSealed": "Building_{namespace}"}}"#).parse()?;
    let document_9 = UncheckedDocument::from_entity(&building_forged_vocab_property_entity_id_seal, Lossless::Normalized).set_member("sealed", &sealed_9);
    let request_9 = broker
        .unchecked(&CreateEntity::builder().entity(&building_forged_vocab_property_entity_id_seal).build())?
        .document(&document_9);
    let answer_9 = broker.send_unchecked(request_9).await;
    expect_that!(&answer_9, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let building_forged_vocab_property_entity_type_se = Entity::builder()
        .id(building_forged_vocab_property_entity_type_se_id)
        .entity_type(building_type.clone())
        .build()?;
    // The model refuses it (member `entityTypeSealed` is not allowed here at sealed), so `sealed` is written as text.
    let sealed_10: RawJson = format!(r#"{{"type": "VocabProperty", "vocab": "sensor", "entityTypeSealed": "Building_{namespace}"}}"#).parse()?;
    let document_10 = UncheckedDocument::from_entity(&building_forged_vocab_property_entity_type_se, Lossless::Normalized).set_member("sealed", &sealed_10);
    let request_10 = broker
        .unchecked(&CreateEntity::builder().entity(&building_forged_vocab_property_entity_type_se).build())?
        .document(&document_10);
    let answer_10 = broker.send_unchecked(request_10).await;
    expect_that!(&answer_10, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let building_forged_list_property_entity_id_seale = Entity::builder()
        .id(building_forged_list_property_entity_id_seale_id)
        .entity_type(building_type.clone())
        .build()?;
    // The model refuses it (member `entityIdSealed` is not allowed here at sealed), so `sealed` is written as text.
    let sealed_11: RawJson = format!(r#"{{"type": "ListProperty", "valueList": [1, 2, 3], "entityIdSealed": "Building_{namespace}"}}"#).parse()?;
    let document_11 = UncheckedDocument::from_entity(&building_forged_list_property_entity_id_seale, Lossless::Normalized).set_member("sealed", &sealed_11);
    let request_11 = broker
        .unchecked(&CreateEntity::builder().entity(&building_forged_list_property_entity_id_seale).build())?
        .document(&document_11);
    let answer_11 = broker.send_unchecked(request_11).await;
    expect_that!(&answer_11, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let building_forged_list_property_entity_type_sea = Entity::builder()
        .id(building_forged_list_property_entity_type_sea_id)
        .entity_type(building_type.clone())
        .build()?;
    // The model refuses it (member `entityTypeSealed` is not allowed here at sealed), so `sealed` is written as text.
    let sealed_12: RawJson = format!(r#"{{"type": "ListProperty", "valueList": [1, 2, 3], "entityTypeSealed": "Building_{namespace}"}}"#).parse()?;
    let document_12 = UncheckedDocument::from_entity(&building_forged_list_property_entity_type_sea, Lossless::Normalized).set_member("sealed", &sealed_12);
    let request_12 = broker
        .unchecked(&CreateEntity::builder().entity(&building_forged_list_property_entity_type_sea).build())?
        .document(&document_12);
    let answer_12 = broker.send_unchecked(request_12).await;
    expect_that!(&answer_12, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let building_forged_list_relationship_entity_id_s = Entity::builder()
        .id(building_forged_list_relationship_entity_id_s_id)
        .entity_type(building_type.clone())
        .build()?;
    // The model refuses it (member `entityIdSealed` is not allowed here at sealed), so `sealed` is written as text.
    let sealed_13: RawJson =
        format!(r#"{{"type": "ListRelationship", "objectList": ["urn:ngsi-ld:Room:{namespace}-1"], "entityIdSealed": "Building_{namespace}"}}"#).parse()?;
    let document_13 = UncheckedDocument::from_entity(&building_forged_list_relationship_entity_id_s, Lossless::Normalized).set_member("sealed", &sealed_13);
    let request_13 = broker
        .unchecked(&CreateEntity::builder().entity(&building_forged_list_relationship_entity_id_s).build())?
        .document(&document_13);
    let answer_13 = broker.send_unchecked(request_13).await;
    expect_that!(&answer_13, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let building_forged_list_relationship_entity_type = Entity::builder()
        .id(building_forged_list_relationship_entity_type_id)
        .entity_type(building_type.clone())
        .build()?;
    // The model refuses it (member `entityTypeSealed` is not allowed here at sealed), so `sealed` is written as text.
    let sealed_14: RawJson =
        format!(r#"{{"type": "ListRelationship", "objectList": ["urn:ngsi-ld:Room:{namespace}-1"], "entityTypeSealed": "Building_{namespace}"}}"#).parse()?;
    let document_14 = UncheckedDocument::from_entity(&building_forged_list_relationship_entity_type, Lossless::Normalized).set_member("sealed", &sealed_14);
    let request_14 = broker
        .unchecked(&CreateEntity::builder().entity(&building_forged_list_relationship_entity_type).build())?
        .document(&document_14);
    let answer_14 = broker.send_unchecked(request_14).await;
    expect_that!(&answer_14, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let building_forged_json_property_entity_id_seale = Entity::builder()
        .id(building_forged_json_property_entity_id_seale_id)
        .entity_type(building_type.clone())
        .build()?;
    // The model refuses it (member `entityIdSealed` is not allowed here at sealed), so `sealed` is written as text.
    let sealed_15: RawJson = format!(r#"{{"type": "JsonProperty", "json": {{"floors": 3}}, "entityIdSealed": "Building_{namespace}"}}"#).parse()?;
    let document_15 = UncheckedDocument::from_entity(&building_forged_json_property_entity_id_seale, Lossless::Normalized).set_member("sealed", &sealed_15);
    let request_15 = broker
        .unchecked(&CreateEntity::builder().entity(&building_forged_json_property_entity_id_seale).build())?
        .document(&document_15);
    let answer_15 = broker.send_unchecked(request_15).await;
    expect_that!(&answer_15, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let building_forged_json_property_entity_type_sea = Entity::builder()
        .id(building_forged_json_property_entity_type_sea_id)
        .entity_type(building_type.clone())
        .build()?;
    // The model refuses it (member `entityTypeSealed` is not allowed here at sealed), so `sealed` is written as text.
    let sealed_16: RawJson = format!(r#"{{"type": "JsonProperty", "json": {{"floors": 3}}, "entityTypeSealed": "Building_{namespace}"}}"#).parse()?;
    let document_16 = UncheckedDocument::from_entity(&building_forged_json_property_entity_type_sea, Lossless::Normalized).set_member("sealed", &sealed_16);
    let request_16 = broker
        .unchecked(&CreateEntity::builder().entity(&building_forged_json_property_entity_type_sea).build())?
        .document(&document_16);
    let answer_16 = broker.send_unchecked(request_16).await;
    expect_that!(&answer_16, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    Ok(())
}
