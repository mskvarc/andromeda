//! An entity with a `VocabProperty` holding one term, and one holding a list of terms, is created and retrieved with
//! them (clauses 5.6.1 and 4.5.20).

use crate::entity_operations::{create_entity_answer, delete_entities, expect_created, retrieve_entity_json};
use andromeda_test_suite::{broker, comparison::matches_document, namespace::Namespace, vocabulary::Vocabulary};
use googletest::prelude::*;
use ngsi_ld::model::{Entity, Input, NgsiLdMediaType, VocabProperty, VocabTerm};
use serde_json::json;

/// The Building `urn:ngsi-ld:Building:<ns>-1` whose `vocabProperty` is the single term `monument`.
fn building_vocab_term(namespace: &Namespace) -> Result<Entity<Input>> {
    Ok(Entity::builder()
        .id(namespace.urn("Building", 1)?)
        .entity_type("Building")
        .vocab_property("vocabProperty", "monument")
        .build()?)
}

/// The Building `urn:ngsi-ld:Building:<ns>-1` whose `vocabProperty` is the terms `monument` and `sightseeing`.
fn building_vocab_terms(namespace: &Namespace) -> Result<Entity<Input>> {
    let terms = VocabProperty::builder(vec!["monument".parse::<VocabTerm>()?, "sightseeing".parse::<VocabTerm>()?]).build()?;
    Ok(Entity::builder()
        .id(namespace.urn("Building", 1)?)
        .entity_type("Building")
        .vocab_property("vocabProperty", terms)
        .build()?)
}

/// Pins: Check that you can create an entity with a `VocabProperty` property
/// Clause: 5.6.1, 4.5.20
#[gtest]
#[tokio::test]
async fn e001_15_01_entity_with_vocab_property_as_string() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Building", "vocabProperty"]).await?;
    let entity = building_vocab_term(&namespace)?;
    let created = create_entity_answer(&broker, &vocabulary, &entity).await?;
    expect_created(&created, &entity.id);
    let client = broker.with_context(vocabulary.context())?.with_accept(NgsiLdMediaType::JsonLd);
    let body = retrieve_entity_json(&client, &entity.id).await?;
    expect_that!(
        &body,
        matches_document(json!({
            "id": entity.id.to_string(),
            "type": "Building",
            "vocabProperty": {"type": "VocabProperty", "vocab": "monument"},
            "@context": vocabulary.url().as_str(),
        }))
    );
    delete_entities(&broker, &[&entity.id]).await;
    Ok(())
}

/// Pins: Check that you can create an entity with a `VocabProperty` property
/// Clause: 5.6.1, 4.5.20
#[gtest]
#[tokio::test]
async fn e001_15_02_entity_with_vocab_property_as_array() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Building", "vocabProperty"]).await?;
    let entity = building_vocab_terms(&namespace)?;
    let created = create_entity_answer(&broker, &vocabulary, &entity).await?;
    expect_created(&created, &entity.id);
    let client = broker.with_context(vocabulary.context())?.with_accept(NgsiLdMediaType::JsonLd);
    let body = retrieve_entity_json(&client, &entity.id).await?;
    expect_that!(
        &body,
        matches_document(json!({
            "id": entity.id.to_string(),
            "type": "Building",
            "vocabProperty": {"type": "VocabProperty", "vocab": ["monument", "sightseeing"]},
            "@context": vocabulary.url().as_str(),
        }))
    );
    delete_entities(&broker, &[&entity.id]).await;
    Ok(())
}
