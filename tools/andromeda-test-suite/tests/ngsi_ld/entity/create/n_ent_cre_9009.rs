//! An absent value or an empty member of another family is refused.

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

/// Pins: Create Entity with a Property naming no value, a Relationship with object `[]`, a `VocabProperty` with
/// vocab `[]`, a `LanguageProperty` with `languageMap` {} or a `GeoProperty` with value `[]` answers 400
/// `BadRequestData` and stores nothing
/// Clause: Table 5.2.5-1, 4.5.3.2, 4.5.18.2, 4.5.20.2, 4.7.1, 5.6.1.4
#[gtest]
#[tokio::test]
async fn n_ent_cre_9009_an_absent_value_or_an_empty_member_of_another_family_is_refused() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let empty_probe_no_value_id = namespace.urn("EmptyProbe", "no-value")?;
    let empty_probe_type = namespace.entity_type("EmptyProbe")?;
    let empty_probe_no_object_id = namespace.urn("EmptyProbe", "no-object")?;
    let empty_probe_no_vocab_id = namespace.urn("EmptyProbe", "no-vocab")?;
    let empty_probe_no_language_id = namespace.urn("EmptyProbe", "no-language")?;
    let empty_probe_no_geometry_id = namespace.urn("EmptyProbe", "no-geometry")?;
    let empty_probe_no_value = Entity::builder()
        .id(empty_probe_no_value_id.clone())
        .entity_type(empty_probe_type.clone())
        .build()?;
    // The model refuses it (missing member `value` at tags), so `tags` is written as text.
    let tags: RawJson = r#"{"type": "Property"}"#.parse()?;
    let document = UncheckedDocument::from_entity(&empty_probe_no_value, Lossless::Normalized).set_member("tags", &tags);
    let request = broker
        .unchecked(&CreateEntity::builder().entity(&empty_probe_no_value).build())?
        .document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&empty_probe_no_value_id)
        .send()
        .await;
    expect_that!(&answer_2, answered(StatusCode::NOT_FOUND));
    let empty_probe_no_object = Entity::builder()
        .id(empty_probe_no_object_id.clone())
        .entity_type(empty_probe_type.clone())
        .build()?;
    // The model refuses it (the member cannot be empty at tags), so `tags` is written as text.
    let tags_2: RawJson = r#"{"type": "Relationship", "object": []}"#.parse()?;
    let document_2 = UncheckedDocument::from_entity(&empty_probe_no_object, Lossless::Normalized).set_member("tags", &tags_2);
    let request_2 = broker
        .unchecked(&CreateEntity::builder().entity(&empty_probe_no_object).build())?
        .document(&document_2);
    let answer_3 = broker.send_unchecked(request_2).await;
    expect_that!(&answer_3, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_4 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&empty_probe_no_object_id)
        .send()
        .await;
    expect_that!(&answer_4, answered(StatusCode::NOT_FOUND));
    let empty_probe_no_vocab = Entity::builder()
        .id(empty_probe_no_vocab_id.clone())
        .entity_type(empty_probe_type.clone())
        .build()?;
    // The model refuses it (the member cannot be empty at tags), so `tags` is written as text.
    let tags_3: RawJson = r#"{"type": "VocabProperty", "vocab": []}"#.parse()?;
    let document_3 = UncheckedDocument::from_entity(&empty_probe_no_vocab, Lossless::Normalized).set_member("tags", &tags_3);
    let request_3 = broker
        .unchecked(&CreateEntity::builder().entity(&empty_probe_no_vocab).build())?
        .document(&document_3);
    let answer_5 = broker.send_unchecked(request_3).await;
    expect_that!(&answer_5, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_6 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&empty_probe_no_vocab_id)
        .send()
        .await;
    expect_that!(&answer_6, answered(StatusCode::NOT_FOUND));
    let empty_probe_no_language = Entity::builder()
        .id(empty_probe_no_language_id.clone())
        .entity_type(empty_probe_type.clone())
        .build()?;
    // The model refuses it (invalid language map at tags), so `tags` is written as text.
    let tags_4: RawJson = r#"{"type": "LanguageProperty", "languageMap": {}}"#.parse()?;
    let document_4 = UncheckedDocument::from_entity(&empty_probe_no_language, Lossless::Normalized).set_member("tags", &tags_4);
    let request_4 = broker
        .unchecked(&CreateEntity::builder().entity(&empty_probe_no_language).build())?
        .document(&document_4);
    let answer_7 = broker.send_unchecked(request_4).await;
    expect_that!(&answer_7, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_8 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&empty_probe_no_language_id)
        .send()
        .await;
    expect_that!(&answer_8, answered(StatusCode::NOT_FOUND));
    let empty_probe_no_geometry = Entity::builder()
        .id(empty_probe_no_geometry_id.clone())
        .entity_type(empty_probe_type.clone())
        .build()?;
    // The model refuses it (invalid geometry at tags), so `tags` is written as text.
    let tags_5: RawJson = r#"{"type": "GeoProperty", "value": []}"#.parse()?;
    let document_5 = UncheckedDocument::from_entity(&empty_probe_no_geometry, Lossless::Normalized).set_member("tags", &tags_5);
    let request_5 = broker
        .unchecked(&CreateEntity::builder().entity(&empty_probe_no_geometry).build())?
        .document(&document_5);
    let answer_9 = broker.send_unchecked(request_5).await;
    expect_that!(&answer_9, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_10 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&empty_probe_no_geometry_id)
        .send()
        .await;
    expect_that!(&answer_10, answered(StatusCode::NOT_FOUND));
    Ok(())
}
