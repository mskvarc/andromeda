//! ETSI `001_10`: an entity with a `LanguageProperty` is created and retrieved with its language map (clause 5.6.1).

use crate::suite_keywords::{create_entity_answer, delete_entities, expect_created, retrieve_entity_json};
use andromeda_test_suite::{broker, comparison::matches_document, namespace::Namespace, vocabulary::Vocabulary};
use googletest::prelude::*;
use ngsi_ld::model::{Entity, Input, NgsiLdMediaType};
use serde_json::json;

/// The Building `urn:ngsi-ld:Building:<ns>-1` whose `street` is a `LanguageProperty` in French and Dutch.
fn building_with_street(namespace: &Namespace) -> Result<Entity<Input>> {
    Ok(Entity::builder()
        .id(namespace.urn("Building", 1)?)
        .entity_type("Building")
        .language_property("street", [("fr", "Grand Place"), ("nl", "Grote Markt")])
        .build()?)
}

/// Pins: Check that one can create an entity with a `LanguageProperty` property
/// Clause: 5.6.1, 4.5.18
/// Changed: Check Response Headers Containing URI set to checks Location only when the response header dictionary
/// holds a key spelled exactly `Location`, which a broker sending a lower-case `location` (valid per RFC 9110, and what
/// Andromeda sends) never satisfies; Hurl matches header names without regard to case and cannot make an assertion
/// conditional, so the port requires a Location header that contains the entity id
#[gtest]
#[tokio::test]
async fn e001_10_01_entity_with_language_property() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Building", "street"]).await?;
    let entity = building_with_street(&namespace)?;
    let created = create_entity_answer(&broker, &vocabulary, &entity).await?;
    expect_created(&created, &entity.id);
    let client = broker.with_context(vocabulary.context())?.with_accept(NgsiLdMediaType::JsonLd);
    let body = retrieve_entity_json(&client, &entity.id).await?;
    expect_that!(
        &body,
        matches_document(json!({
            "id": entity.id.to_string(),
            "type": "Building",
            "street": {
                "type": "LanguageProperty",
                "languageMap": {"fr": "Grand Place", "nl": "Grote Markt"}
            },
            "@context": vocabulary.url().as_str(),
        }))
    );
    delete_entities(&broker, &[&entity.id]).await;
    Ok(())
}
