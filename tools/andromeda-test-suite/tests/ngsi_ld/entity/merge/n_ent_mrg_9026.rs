//! A language merged as several strings replaces every stored string of it.

use andromeda_test_suite::{
    body_json::{json_count, json_equals},
    broker,
    namespace::Namespace,
    problem::answered,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{
        UncheckedDocument,
        headers::Accept,
        operation::{CreateEntity, MergeEntity, RetrieveEntity},
    },
    model::{Entity, EntityFragment, LanguageMap, LanguageProperty, Lossless, NgsiLdMediaType, RawJson},
};

/// Pins: Merge Entity of street with en `["Rue Cler", "Rue Saint-Dominique"]` into a stored street with en
/// `["Champ de Mars", "Quai Branly"]` and fr "Champ de Mars" answers 204, and street then holds en
/// `["Rue Cler", "Rue Saint-Dominique"]` beside the untouched fr
/// Clause: 5.6.17.4, 5.5.12, 4.5.18.2
#[gtest]
#[tokio::test]
async fn n_ent_mrg_9026_a_language_merged_as_several_strings_replaces_every_stored_string_of_it() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_languages_id = namespace.urn("Building", "languages")?;
    let building_languages = Entity::builder()
        .id(building_languages_id.clone())
        .entity_type(namespace.entity_type("Building")?)
        .build()?;
    let stored_street: RawJson = r#"{"type": "LanguageProperty", "languageMap": {"en": ["Champ de Mars", "Quai Branly"], "fr": "Champ de Mars"}}"#.parse()?;
    // The model refuses a language map entry holding several strings, so `street` is written as text beside the entity.
    let request = broker
        .unchecked(&CreateEntity::builder().entity(&building_languages).build())?
        .document(&UncheckedDocument::from_entity(&building_languages, Lossless::Normalized).set_member("street", &stored_street));
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    let placeholder = EntityFragment::builder()
        .language_property("street", LanguageProperty::builder(LanguageMap::try_from_pairs([("en", "Rue Cler")])?))
        .build()?;
    let merged_street: RawJson = r#"{"type": "LanguageProperty", "languageMap": {"en": ["Rue Cler", "Rue Saint-Dominique"]}}"#.parse()?;
    // The model refuses a language map entry holding several strings, so the typed merge carries `street` as text.
    let request_2 = broker
        .unchecked(&MergeEntity::builder().id(&building_languages_id).fragment(&placeholder).build())?
        .document(&UncheckedDocument::empty().set_member("street", &merged_street));
    let answer_2 = broker.send_unchecked(request_2).await;
    expect_that!(&answer_2, answered(StatusCode::NO_CONTENT));
    // The entity reader refuses a language map entry holding several strings, so the answer is read as received.
    let request_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .unchecked(&RetrieveEntity::builder().id(&building_languages_id).format(Lossless::Normalized).build())?;
    let retrieved = broker.send_unchecked(request_3).await?;
    let entity = retrieved.body();
    expect_that!(entity, json_count("$.street.languageMap.en", 2));
    expect_that!(entity, json_equals("$.street.languageMap.en[0]", "Rue Cler"));
    expect_that!(entity, json_equals("$.street.languageMap.en[1]", "Rue Saint-Dominique"));
    expect_that!(entity, json_equals("$.street.languageMap.fr", "Champ de Mars"));
    Ok(())
}
