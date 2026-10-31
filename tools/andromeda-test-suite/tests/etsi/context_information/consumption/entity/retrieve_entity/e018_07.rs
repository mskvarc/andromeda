//! A `lang` filter reduces a `LanguageProperty` to the language it names, or to the value of its first preferred
//! language, in the normalized and the simplified representation (clauses 5.7.1 and 4.15).

use crate::entity_operations::create_entity;
use andromeda_test_suite::{
    broker::{self, BrokerClient},
    comparison::matches_document,
    namespace::Namespace,
    problem::answered,
    teardown::AnyAnswer,
    vocabulary::Vocabulary,
    wire::json_body,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    model::{Entity, Input, NgsiLdMediaType},
    query::{
        Quality,
        language_filter::{self, LanguageFilter},
    },
};
use serde_json::{Map, Value, json};

/// The terms of the case's `@context`.
const TERMS: [&str; 2] = ["Building", "street"];

/// The case's building, whose `street` is a `LanguageProperty` in French and Dutch.
fn street_building(namespace: &Namespace) -> Result<Entity<Input>> {
    Ok(Entity::builder()
        .id(namespace.urn("Building", 1)?)
        .entity_type("Building")
        .language_property("street", [("fr", "Grand Place"), ("nl", "Grote Markt")])
        .build()?)
}

/// Retrieves `building` in the normalized representation, reduced by `lang`, with the `Link` header naming
/// `vocabulary`, and answers its body.
async fn normalized(broker: &BrokerClient, vocabulary: &Vocabulary, building: &Entity<Input>, lang: LanguageFilter) -> Result<Value> {
    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(NgsiLdMediaType::JsonLd)
        .with_responses()
        .entities()
        .retrieve(&building.id)
        .lang(lang)
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    Ok(json_body(&received?)?)
}

/// Retrieves `building` in the simplified representation, reduced by `lang`, with the `Link` header naming
/// `vocabulary`, and answers its body.
async fn simplified(broker: &BrokerClient, vocabulary: &Vocabulary, building: &Entity<Input>, lang: LanguageFilter) -> Result<Value> {
    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(NgsiLdMediaType::JsonLd)
        .with_responses()
        .entities()
        .retrieve_simplified(&building.id)
        .lang(lang)
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    Ok(json_body(&received?)?)
}

/// The expected normalized building, its `street` reduced to the French text.
fn french_street_normalized(building: &Entity<Input>, vocabulary: &Vocabulary) -> Value {
    json!({
        "id": building.id.to_string(),
        "type": "Building",
        "street": {"type": "Property", "value": "Grand Place", "lang": "fr"},
        "@context": vocabulary.url().as_str(),
    })
}

/// Pins: Check that one can retrieve an entity using Language Filter
/// Clause: 5.7.1, 4.15
#[gtest]
#[tokio::test]
async fn e018_07_01_retrieve_an_entity_using_a_specific_natural_language() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &TERMS).await?;
    let building = street_building(&namespace)?;
    create_entity(&broker, &vocabulary, &building).await?;
    let body = normalized(&broker, &vocabulary, &building, language_filter::language("fr").build()?).await?;
    expect_that!(&body, matches_document(french_street_normalized(&building, &vocabulary)).hiding("instanceId"));
    expect_that!(body.as_object().map(Map::len), some(eq(4)));
    broker.entities().delete(&building.id).send().await.any_answer();
    broker.temporal().delete(&building.id).send().await.any_answer();
    Ok(())
}

/// Pins: Check that one can retrieve an entity using Language Filter
/// Clause: 5.7.1, 4.15
#[gtest]
#[tokio::test]
async fn e018_07_02_retrieve_an_entity_using_multiple_natural_languages_with_no_ranked_preference() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &TERMS).await?;
    let building = street_building(&namespace)?;
    create_entity(&broker, &vocabulary, &building).await?;
    let filter = language_filter::language("fr-CH").language("fr").build()?;
    let body = normalized(&broker, &vocabulary, &building, filter).await?;
    expect_that!(&body, matches_document(french_street_normalized(&building, &vocabulary)).hiding("instanceId"));
    expect_that!(body.as_object().map(Map::len), some(eq(4)));
    broker.entities().delete(&building.id).send().await.any_answer();
    broker.temporal().delete(&building.id).send().await.any_answer();
    Ok(())
}

/// Pins: Check that one can retrieve an entity using Language Filter
/// Clause: 5.7.1, 4.15
#[gtest]
#[tokio::test]
async fn e018_07_03_retrieve_an_entity_with_any_supported_language() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &TERMS).await?;
    let building = street_building(&namespace)?;
    create_entity(&broker, &vocabulary, &building).await?;
    let body = normalized(&broker, &vocabulary, &building, language_filter::any().build()?).await?;
    expect_that!(&body, matches_document(french_street_normalized(&building, &vocabulary)).hiding("instanceId"));
    expect_that!(body.as_object().map(Map::len), some(eq(4)));
    broker.entities().delete(&building.id).send().await.any_answer();
    broker.temporal().delete(&building.id).send().await.any_answer();
    Ok(())
}

/// Pins: Check that one can retrieve an entity using Language Filter
/// Clause: 5.7.1, 4.15
#[gtest]
#[tokio::test]
async fn e018_07_04_retrieve_an_entity_using_multiple_natural_languages_with_ranked_preferences() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &TERMS).await?;
    let building = street_building(&namespace)?;
    create_entity(&broker, &vocabulary, &building).await?;
    let filter = language_filter::language("fr-CH")
        .weighted_language("fr", Quality::try_from_thousandths(900)?)
        .weighted_language("en", Quality::try_from_thousandths(800)?)
        .weighted_any(Quality::try_from_thousandths(500)?)
        .build()?;
    let body = normalized(&broker, &vocabulary, &building, filter).await?;
    expect_that!(&body, matches_document(french_street_normalized(&building, &vocabulary)).hiding("instanceId"));
    expect_that!(body.as_object().map(Map::len), some(eq(4)));
    broker.entities().delete(&building.id).send().await.any_answer();
    broker.temporal().delete(&building.id).send().await.any_answer();
    Ok(())
}

/// Pins: Check that one can retrieve an entity using Language Filter
/// Clause: 5.7.1, 4.15
#[gtest]
#[tokio::test]
async fn e018_07_05_retrieve_an_entity_using_a_specific_natural_language_with_simplified_representation() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &TERMS).await?;
    let building = street_building(&namespace)?;
    create_entity(&broker, &vocabulary, &building).await?;
    let body = simplified(&broker, &vocabulary, &building, language_filter::language("fr").build()?).await?;
    let expected = json!({
        "id": building.id.to_string(),
        "type": "Building",
        "street": "Grand Place",
        "@context": vocabulary.url().as_str(),
    });
    expect_that!(&body, matches_document(expected));
    broker.entities().delete(&building.id).send().await.any_answer();
    broker.temporal().delete(&building.id).send().await.any_answer();
    Ok(())
}

/// Pins: Check that one can retrieve an entity using Language Filter
/// Clause: 5.7.1, 4.15
#[gtest]
#[tokio::test]
async fn e018_07_06_retrieve_an_entity_with_any_supported_language_with_simplified_representation() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &TERMS).await?;
    let building = street_building(&namespace)?;
    create_entity(&broker, &vocabulary, &building).await?;
    let body = simplified(&broker, &vocabulary, &building, language_filter::any().build()?).await?;
    let expected = json!({
        "id": building.id.to_string(),
        "type": "Building",
        "street": "Grand Place",
        "@context": vocabulary.url().as_str(),
    });
    expect_that!(&body, matches_document(expected));
    broker.entities().delete(&building.id).send().await.any_answer();
    broker.temporal().delete(&building.id).send().await.any_answer();
    Ok(())
}
