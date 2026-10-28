//! ETSI `012_09`: a partial attribute update of a `VocabProperty` replaces its `vocab` and adds its `providedBy`
//! Relationship, and keeps its `source` sub-attribute (clauses 5.6.4 and 4.5.20).

use crate::{
    suite_keywords::{create_entity, delete_entities, ld_json_body_client, partial_update_request, retrieve_entity_json},
    suite_payloads::{BUILDING_VOCAB_PROPERTY_TERMS, provided_by_john_doe},
};
use andromeda_test_suite::{broker, comparison::matches_document, namespace::Namespace, problem::answered, vocabulary::Vocabulary};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::model::{AttributeName, Entity, Input, NgsiLdMediaType, Property, VocabProperty, VocabTerm};
use serde_json::json;

/// The Building `urn:ngsi-ld:Vehicle:<ns>-1` of the setup: a `vocabProperty` of `monument` with a `source` Property of
/// Speedometer.
fn building_with_vocab_property(namespace: &Namespace) -> Result<Entity<Input>> {
    Ok(Entity::builder()
        .id(namespace.urn("Vehicle", 1)?)
        .entity_type("Building")
        .vocab_property(
            "vocabProperty",
            VocabProperty::builder("monument".parse::<VocabTerm>()?).property("source", Property::builder("Speedometer")),
        )
        .build()?)
}

/// Pins: Check that you can perform a partial update on a `VocabProperty` property
/// Clause: 5.6.4, 4.5.20
#[gtest]
#[tokio::test]
async fn e012_09_01_check_that_you_can_partially_update_a_vocab_property_property() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &BUILDING_VOCAB_PROPERTY_TERMS).await?;
    let id = namespace.urn("Vehicle", 1)?;
    create_entity(&broker, &vocabulary, &building_with_vocab_property(&namespace)?).await?;
    let vocab_property: AttributeName = "vocabProperty".parse()?;
    let document = provided_by_john_doe(&namespace)?.set_text("vocab", "sightseeing");
    let client = ld_json_body_client(&broker, &vocabulary)?;
    let answer = client.send_unchecked(partial_update_request(&client, &id, &vocab_property, &document)?).await;
    verify_that!(&answer, answered(StatusCode::NO_CONTENT))?;
    let read = broker.with_context(vocabulary.context())?.with_accept(NgsiLdMediaType::JsonLd);
    let body = retrieve_entity_json(&read, &id).await?;
    expect_that!(
        &body,
        matches_document(json!({
            "id": id.to_string(),
            "type": "Building",
            "vocabProperty": {
                "type": "VocabProperty",
                "vocab": "sightseeing",
                "source": {"type": "Property", "value": "Speedometer"},
                "providedBy": {"type": "Relationship", "object": namespace.urn_text("Person", "JohnDoe")},
            },
        }))
        .hiding_everywhere("@context")
    );
    delete_entities(&broker, &[&id]).await;
    Ok(())
}
