//! ETSI `001_14`: an entity with a `JsonProperty` whose `json` is an object, and one whose `json` is an array of
//! objects, is created and retrieved with that JSON as written (clauses 5.6.1 and 4.5.24).

use crate::suite_keywords::{create_entity_answer, delete_entities, expect_created, retrieve_entity_json};
use andromeda_test_suite::{broker, comparison::matches_document, namespace::Namespace, vocabulary::Vocabulary};
use googletest::prelude::*;
use ngsi_ld::model::{Entity, Input, NgsiLdMediaType};
use serde_json::json;

/// The Building `urn:ngsi-ld:Building:<ns>-1` whose `jsonProperty` holds the object `json`.
fn building_json_object(namespace: &Namespace) -> Result<Entity<Input>> {
    Ok(Entity::builder()
        .id(namespace.urn("Building", 1)?)
        .entity_type("Building")
        .json_property(
            "jsonProperty",
            r#"{"id": "some-id", "type": "some-type", "null": null, "anythingElse": "anything-else"}"#,
        )
        .build()?)
}

/// The Building `urn:ngsi-ld:Building:<ns>-1` whose `jsonProperty` holds the array `json` of two objects.
fn building_json_array(namespace: &Namespace) -> Result<Entity<Input>> {
    Ok(Entity::builder()
        .id(namespace.urn("Building", 1)?)
        .entity_type("Building")
        .json_property(
            "jsonProperty",
            r#"[{"id": "some-id-1", "anythingElse": "anything-else-1"}, {"id": "some-id-2", "anythingElse": "anything-else-2"}]"#,
        )
        .build()?)
}

/// Pins: Check that you can create an entity with a `JsonProperty` property
/// Clause: 5.6.1, 4.5.24
/// Changed: Check Response Headers Containing URI set to checks Location only when the response header dictionary
/// holds a key spelled exactly `Location`, which a broker sending a lower-case `location` (valid per RFC 9110, and what
/// Andromeda sends) never satisfies; Hurl matches header names without regard to case and cannot make an assertion
/// conditional, so the port requires a Location header that contains the entity id
#[gtest]
#[tokio::test]
async fn e001_14_01_entity_with_json_property_as_object() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Building", "jsonProperty"]).await?;
    let entity = building_json_object(&namespace)?;
    let created = create_entity_answer(&broker, &vocabulary, &entity).await?;
    expect_created(&created, &entity.id);
    let client = broker.with_context(vocabulary.context())?.with_accept(NgsiLdMediaType::JsonLd);
    let body = retrieve_entity_json(&client, &entity.id).await?;
    expect_that!(
        &body,
        matches_document(json!({
            "id": entity.id.to_string(),
            "type": "Building",
            "jsonProperty": {
                "type": "JsonProperty",
                "json": {"id": "some-id", "type": "some-type", "null": null, "anythingElse": "anything-else"}
            },
            "@context": vocabulary.url().as_str(),
        }))
    );
    delete_entities(&broker, &[&entity.id]).await;
    Ok(())
}

/// Pins: Check that you can create an entity with a `JsonProperty` property
/// Clause: 5.6.1, 4.5.24
/// Changed: Check Response Headers Containing URI set to checks Location only when the response header dictionary
/// holds a key spelled exactly `Location`, which a broker sending a lower-case `location` (valid per RFC 9110, and what
/// Andromeda sends) never satisfies; Hurl matches header names without regard to case and cannot make an assertion
/// conditional, so the port requires a Location header that contains the entity id
#[gtest]
#[tokio::test]
async fn e001_14_02_entity_with_json_property_as_array() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Building", "jsonProperty"]).await?;
    let entity = building_json_array(&namespace)?;
    let created = create_entity_answer(&broker, &vocabulary, &entity).await?;
    expect_created(&created, &entity.id);
    let client = broker.with_context(vocabulary.context())?.with_accept(NgsiLdMediaType::JsonLd);
    let body = retrieve_entity_json(&client, &entity.id).await?;
    expect_that!(
        &body,
        matches_document(json!({
            "id": entity.id.to_string(),
            "type": "Building",
            "jsonProperty": {
                "type": "JsonProperty",
                "json": [
                    {"id": "some-id-1", "anythingElse": "anything-else-1"},
                    {"id": "some-id-2", "anythingElse": "anything-else-2"}
                ]
            },
            "@context": vocabulary.url().as_str(),
        }))
    );
    delete_entities(&broker, &[&entity.id]).await;
    Ok(())
}
