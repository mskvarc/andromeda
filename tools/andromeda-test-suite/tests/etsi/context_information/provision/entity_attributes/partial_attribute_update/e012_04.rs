//! A partial attribute update replaces the languageMap of a `LanguageProperty` and adds a sub-attribute to it, keeping
//! the sub-attributes the update does not name (clauses 5.6.4 and 4.5.18).

use crate::entity_operations::{create_entity, delete_entities, ld_json_body_client, partial_update, retrieve_entity_json};
use andromeda_test_suite::{broker, comparison::matches_document, namespace::Namespace, vocabulary::Vocabulary};
use googletest::prelude::*;
use ngsi_ld::model::{Attribute, AttributeName, Change, Entity, Input, LanguageMap, LanguageProperty, Lossless, NgsiLdMediaType, Property};
use serde_json::json;

/// The Building `urn:ngsi-ld:Vehicle:<ns>-1` of the setup: a `street` `LanguageProperty` in French and Dutch whose
/// `source` is a Speedometer.
fn building_with_street_source(namespace: &Namespace) -> Result<Entity<Input>> {
    Ok(Entity::builder()
        .id(namespace.urn("Vehicle", 1)?)
        .entity_type("Building")
        .language_property(
            "street",
            LanguageProperty::builder(LanguageMap::try_from_pairs([("fr", "Grand Place"), ("nl", "Grote Markt")])?)
                .property("source", Property::builder("Speedometer")),
        )
        .build()?)
}

/// Pins: Check that one can perform a partial update on a `LanguageProperty` property
/// Clause: 5.6.4, 4.5.18
#[gtest]
#[tokio::test]
async fn e012_04_01_check_that_one_can_partially_update_a_language_property_property() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Building", "street", "source", "providedBy"]).await?;
    create_entity(&broker, &vocabulary, &building_with_street_source(&namespace)?).await?;
    let id = namespace.urn("Vehicle", 1)?;
    let street: AttributeName = "street".parse()?;
    let updated = LanguageProperty::builder(LanguageMap::try_from_pairs([("fr", "Grand Place"), ("es", "Gran Lugar")])?)
        .relationship("providedBy", namespace.urn("Person", "JohnDoe")?);
    let client = ld_json_body_client(&broker, &vocabulary)?;
    partial_update(
        &client,
        &id,
        &street,
        Change::from(Attribute::from(updated.build()?).into_patch()),
        Lossless::Normalized,
    )
    .await?;
    let read = broker.with_context(vocabulary.context())?.with_accept(NgsiLdMediaType::JsonLd);
    let body = retrieve_entity_json(&read, &id).await?;
    expect_that!(
        &body,
        matches_document(json!({
            "id": id.to_string(),
            "type": "Building",
            "street": {
                "type": "LanguageProperty",
                "languageMap": {"fr": "Grand Place", "es": "Gran Lugar"},
                "providedBy": {"type": "Relationship", "object": namespace.urn_text("Person", "JohnDoe")},
                "source": {"type": "Property", "value": "Speedometer"},
            },
        }))
        .hiding_everywhere("@context")
    );
    delete_entities(&broker, &[&id]).await;
    Ok(())
}
