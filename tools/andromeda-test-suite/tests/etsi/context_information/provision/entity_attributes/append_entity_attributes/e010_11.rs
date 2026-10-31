//! A `VocabProperty` whose `vocab` is a term is appended to a Vehicle that holds no attribute (clauses 5.6.3 and
//! 4.5.20).

use crate::entity_operations::{append_attributes, create_entity, delete_entities, ld_json_body_client, retrieve_entity_json};
use andromeda_test_suite::{broker, comparison::matches_document, namespace::Namespace, vocabulary::Vocabulary};
use googletest::prelude::*;
use ngsi_ld::{
    http::operation::Overwrite,
    model::{Entity, EntityFragment, Input, NgsiLdMediaType, VocabProperty, VocabTerm},
};
use serde_json::json;

/// The terms of the `@context` the case registers.
const TERMS: [&str; 2] = ["Vehicle", "vocabProperty"];

/// Pins: Check that you can append a `VocabProperty` property to an entity
/// Clause: 5.6.3, 4.5.20
#[gtest]
#[tokio::test]
async fn e010_11_01_append_a_vocab_property_property() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &TERMS).await?;
    let original: Entity<Input> = Entity::builder().id(namespace.urn("Vehicle", 1)?).entity_type("Vehicle").build()?;
    create_entity(&broker, &vocabulary, &original).await?;
    let fragment = EntityFragment::builder()
        .vocab_property("vocabProperty", VocabProperty::builder("monument".parse::<VocabTerm>()?))
        .build()?;
    append_attributes(&ld_json_body_client(&broker, &vocabulary)?, &original.id, &fragment, Overwrite::Replace).await?;
    let client = broker.with_context(vocabulary.context())?.with_accept(NgsiLdMediaType::JsonLd);
    let body = retrieve_entity_json(&client, &original.id).await?;
    expect_that!(
        &body,
        matches_document(json!({
            "id": original.id.to_string(),
            "type": "Vehicle",
            "vocabProperty": {"type": "VocabProperty", "vocab": "monument"},
            "@context": vocabulary.url().as_str(),
        }))
    );
    delete_entities(&broker, &[&original.id]).await;
    Ok(())
}
