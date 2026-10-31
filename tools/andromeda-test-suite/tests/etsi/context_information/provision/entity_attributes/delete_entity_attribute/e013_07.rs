//! A `VocabProperty` property is deleted from an entity (clauses 5.6.5 and 4.5.20).

use crate::entity_operations::{create_entity, delete_entities, retrieve_entity_json};
use andromeda_test_suite::{broker, comparison::matches_document, namespace::Namespace, problem::answered, vocabulary::Vocabulary};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::operation::InstanceSelection,
    model::{AttributeName, Entity, NgsiLdMediaType},
};
use serde_json::json;

/// The terms of the `@context` the case registers.
const TERMS: [&str; 1] = ["Building"];

/// Pins: Check that you can delete a `VocabProperty` property from an entity
/// Clause: 5.6.5, 4.5.20
/// Expected: vocabProperty stays out of the file vocabulary: the test reads it without a @context, so it expands under
/// the core default vocabulary
#[gtest]
#[tokio::test]
async fn e013_07_01_delete_a_vocab_property_property_from_an_entity() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &TERMS).await?;
    let id = namespace.urn("Building", 1)?;
    let building = Entity::builder()
        .id(namespace.urn("Building", 1)?)
        .entity_type("Building")
        .vocab_property("vocabProperty", "monument")
        .build()?;
    create_entity(&broker, &vocabulary, &building).await?;
    let vocab_property: AttributeName = "vocabProperty".parse()?;

    let answer = broker
        .with_responses()
        .entities()
        .delete_attribute(&id, &vocab_property)
        .instances(InstanceSelection::Selected(None))
        .send()
        .await;
    verify_that!(&answer, answered(StatusCode::NO_CONTENT))?;

    let body = retrieve_entity_json(&broker.with_accept(NgsiLdMediaType::Json), &id).await?;
    expect_that!(
        &body,
        matches_document(json!({
            "id": id.to_string(),
            "type": namespace.expanded_term("Building")?,
        }))
    );

    delete_entities(&broker, &[&id]).await;
    Ok(())
}
