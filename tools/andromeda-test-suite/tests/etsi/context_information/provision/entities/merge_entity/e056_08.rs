//! A `LanguageProperty` is removed by a Merge Entity whose languageMap holds the NGSI-LD Null, and its languageMap
//! entries are merged, replaced and removed one by one (clauses 5.6.17, 6.5.3.4 and 4.5.18).

use crate::entity_operations::{create_entity, delete_entities, merge_entity, retrieve_entity_json};
use andromeda_test_suite::{broker, comparison::matches_document, namespace::Namespace, vocabulary::Vocabulary};
use googletest::prelude::*;
use ngsi_ld::{
    http::ContextDelivery,
    model::{AttributeKind, Entity, EntityFragment, Input, LanguageMap, LanguageProperty, NgsiLdMediaType},
};
use serde_json::json;

/// The terms of the `@context` the cases register.
const TERMS: [&str; 2] = ["Building", "street"];

/// The Building `urn:ngsi-ld:Building:<ns>-1` of the setup: a `street` `LanguageProperty` in French and Dutch.
fn building_with_street(namespace: &Namespace) -> Result<Entity<Input>> {
    Ok(Entity::builder()
        .id(namespace.urn("Building", 1)?)
        .entity_type("Building")
        .language_property(
            "street",
            LanguageProperty::builder(LanguageMap::try_from_pairs([("fr", "Grand Place"), ("nl", "Grote Markt")])?),
        )
        .build()?)
}

/// Pins: Check that `LanguageProperty` entries can be merged and removed via NGSI-LD Null
/// Clause: 5.6.17, 6.5.3.4, 4.5.18
#[gtest]
#[tokio::test]
async fn e056_08_01_remove_language_map_property() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &TERMS).await?;
    let original = building_with_street(&namespace)?;
    create_entity(&broker, &vocabulary, &original).await?;
    let fragment = EntityFragment::for_entity(namespace.urn("Building", 1)?, "Building")
        .delete_kind("street", AttributeKind::LanguageProperty)
        .build()?;
    let client = broker.with_context(vocabulary.context())?.with_context_delivery(ContextDelivery::LdJsonBody);
    merge_entity(&client, &original.id, &fragment).await?;
    let client = broker.with_context(vocabulary.context())?.with_accept(NgsiLdMediaType::JsonLd);
    let body = retrieve_entity_json(&client, &original.id).await?;
    expect_that!(
        &body,
        matches_document(json!({
            "id": namespace.urn_text("Building", 1),
            "type": "Building",
            "@context": vocabulary.url().as_str(),
        }))
        .hiding("instanceId")
    );
    delete_entities(&broker, &[&original.id]).await;
    Ok(())
}

/// Pins: Check that `LanguageProperty` entries can be merged and removed via NGSI-LD Null
/// Clause: 5.6.17, 6.5.3.4, 4.5.18
#[gtest]
#[tokio::test]
async fn e056_08_02_merge_language_map_property_entries() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &TERMS).await?;
    let original = building_with_street(&namespace)?;
    create_entity(&broker, &vocabulary, &original).await?;
    let street = LanguageProperty::builder(LanguageMap::try_from_pairs([
        ("de", "Grosser Platz"),
        ("fr", "Grande Place"),
        ("nl", "urn:ngsi-ld:null"),
    ])?);
    let fragment = EntityFragment::for_entity(namespace.urn("Building", 1)?, "Building")
        .language_property("street", street)
        .build()?;
    let client = broker.with_context(vocabulary.context())?.with_context_delivery(ContextDelivery::LdJsonBody);
    merge_entity(&client, &original.id, &fragment).await?;
    let client = broker.with_context(vocabulary.context())?.with_accept(NgsiLdMediaType::JsonLd);
    let body = retrieve_entity_json(&client, &original.id).await?;
    expect_that!(
        &body,
        matches_document(json!({
            "id": namespace.urn_text("Building", 1),
            "type": "Building",
            "street": {
                "type": "LanguageProperty",
                "languageMap": {"de": "Grosser Platz", "fr": "Grande Place"},
            },
            "@context": vocabulary.url().as_str(),
        }))
        .hiding("instanceId")
    );
    delete_entities(&broker, &[&original.id]).await;
    Ok(())
}
