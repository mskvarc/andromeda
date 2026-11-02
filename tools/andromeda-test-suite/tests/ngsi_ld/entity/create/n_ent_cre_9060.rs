//! A language map entry that is neither a string nor an array is refused naming its tag.

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

/// Pins: Create Entity with a `LanguageProperty` whose `languageMap` is {"en": 42} answers 400 `BadRequestData`
/// whose detail names `languageMap`, the tag `en` and that a string or an array of strings was expected, carries no
/// decoder vocabulary, and stores nothing
/// Clause: 4.5.18, 5.2.32, 5.6.1.4
#[gtest]
#[tokio::test]
async fn n_ent_cre_9060_a_language_map_entry_that_is_neither_a_string_nor_an_array_is_refused_naming_its_tag() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let street_id = namespace.urn("Street", "numeric-entry")?;
    let street = Entity::builder().id(street_id.clone()).entity_type(namespace.entity_type("Street")?).build()?;
    // The model refuses it (a language map entry is a string or an array of strings), so `name` is written as text.
    let name: RawJson = r#"{"type": "LanguageProperty", "languageMap": {"en": 42}}"#.parse()?;
    let document = UncheckedDocument::from_entity(&street, Lossless::Normalized).set_member("name", &name);
    let request = broker.unchecked(&CreateEntity::builder().entity(&street).build())?.document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(
        &answer,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData)
            .with_detail_containing("'languageMap'")
            .with_detail_containing("a string or an array of strings under the language tag 'en'")
            .with_detail_lacking("untagged enum")
            .with_detail_lacking("variant")
            .with_detail_lacking("LanguageMapValue")
    );
    let retrieved = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&street_id)
        .send()
        .await;
    expect_that!(&retrieved, answered(StatusCode::NOT_FOUND));
    Ok(())
}
