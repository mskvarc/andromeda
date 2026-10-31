//! A partial attribute update of a `JsonProperty` replaces its `json` value and adds its `providedBy` Relationship, and
//! keeps its `source` sub-attribute (clauses 5.6.4 and 4.5.24).

use crate::{
    building_payloads::BUILDING_JSON_PROPERTY_TERMS,
    entity_operations::{create_entity, delete_entities, ld_json_body_client, partial_update_request, retrieve_entity_json},
    vehicle_entity_payloads::provided_by_john_doe,
};
use andromeda_test_suite::{broker, comparison::matches_document, namespace::Namespace, problem::answered, vocabulary::Vocabulary};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::model::{AttributeName, Entity, Input, JsonProperty, NgsiLdMediaType, Property, RawJson};
use serde_json::json;

/// The Building `urn:ngsi-ld:Vehicle:<ns>-1` of the setup: a `jsonProperty` whose `json` holds an id, a type, a null
/// member and an extra member, with a `source` Property of Speedometer.
fn building_with_json_property(namespace: &Namespace) -> Result<Entity<Input>> {
    let json: RawJson = r#"{"id":"some-id","type":"some-type","null":null,"anythingElse":"anything-else"}"#.parse()?;
    Ok(Entity::builder()
        .id(namespace.urn("Vehicle", 1)?)
        .entity_type("Building")
        .json_property("jsonProperty", JsonProperty::builder(json).property("source", Property::builder("Speedometer")))
        .build()?)
}

/// Pins: Check that you can perform a partial update on a JSON property
/// Clause: 5.6.4, 4.5.24
#[gtest]
#[tokio::test]
async fn e012_08_01_check_that_you_can_partially_update_a_json_property() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &BUILDING_JSON_PROPERTY_TERMS).await?;
    let id = namespace.urn("Vehicle", 1)?;
    create_entity(&broker, &vocabulary, &building_with_json_property(&namespace)?).await?;
    let json_property: AttributeName = "jsonProperty".parse()?;
    let replacement: RawJson = r#"{"id":"another-id","type":"another-type","anythingElse":"another-anything-else"}"#.parse()?;
    let document = provided_by_john_doe(&namespace)?.set_member("json", &replacement);
    let client = ld_json_body_client(&broker, &vocabulary)?;
    let answer = client.send_unchecked(partial_update_request(&client, &id, &json_property, &document)?).await;
    verify_that!(&answer, answered(StatusCode::NO_CONTENT))?;
    let read = broker.with_context(vocabulary.context())?.with_accept(NgsiLdMediaType::JsonLd);
    let body = retrieve_entity_json(&read, &id).await?;
    expect_that!(
        &body,
        matches_document(json!({
            "id": id.to_string(),
            "type": "Building",
            "jsonProperty": {
                "type": "JsonProperty",
                "json": {"id": "another-id", "type": "another-type", "anythingElse": "another-anything-else"},
                "source": {"type": "Property", "value": "Speedometer"},
                "providedBy": {"type": "Relationship", "object": namespace.urn_text("Person", "JohnDoe")},
            },
        }))
        .hiding_everywhere("@context")
    );
    delete_entities(&broker, &[&id]).await;
    Ok(())
}
