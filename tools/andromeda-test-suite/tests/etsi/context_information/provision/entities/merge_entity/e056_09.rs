//! ETSI `056_09`: keys inside the object value of a Property are removed, and replaced, by a Merge Entity whose value
//! holds the NGSI-LD Null for them (clauses 5.6.17 and 6.5.3.4).

use crate::{
    suite_keywords::{LD_JSON, create_entity, delete_entities, retrieve_entity_json},
    suite_payloads::bare_building,
};
use andromeda_test_suite::{
    broker::{self, BrokerClient},
    comparison::matches_document,
    namespace::Namespace,
    problem::answered,
    vocabulary::Vocabulary,
    wire::content_type,
};
use googletest::prelude::*;
use http::StatusCode;
use indexmap::IndexMap;
use ngsi_ld::{
    http::{UncheckedDocument, operation::MergeEntity},
    model::{Entity, EntityFragment, Input, Lossless, NgsiLdMediaType, RawJson, Value},
};
use serde_json::json;

/// The terms of the `@context` the cases register.
const TERMS: [&str; 2] = ["Building", "buildingDetails"];

/// The body's `@context` member: the vocabulary's URL as one JSON string, as the Hurl bodies write it.
fn context_member(vocabulary: &Vocabulary) -> Result<RawJson> {
    Ok(format!("\"{}\"", vocabulary.url()).parse::<RawJson>()?)
}

/// Creates the Building `urn:ngsi-ld:Building:<ns>-1` whose `buildingDetails` is an object holding `category`
/// `commercial`, `floors` 5 and `deprecated` true. Requires 201.
async fn create_building(broker: &BrokerClient, vocabulary: &Vocabulary, namespace: &Namespace) -> Result<()> {
    let details = Value::Object(IndexMap::from([
        ("category".to_owned(), Value::from("commercial")),
        ("floors".to_owned(), Value::from(5)),
        ("deprecated".to_owned(), Value::Boolean(true)),
    ]));
    let building: Entity<Input> = Entity::builder()
        .id(namespace.urn("Building", 1)?)
        .entity_type("Building")
        .property("buildingDetails", details)
        .build()?;
    create_entity(broker, vocabulary, &building).await
}

/// Merges `buildingDetails` set to `details` into the Building `urn:ngsi-ld:Building:<ns>-1`. The fragment is sent as raw
/// JSON because it deletes a member inside the object value with the NGSI-LD Null, which the typed builder cannot
/// express. Requires 204 and an empty body.
async fn merge_building(broker: &BrokerClient, vocabulary: &Vocabulary, namespace: &Namespace, details: &str) -> Result<()> {
    let id = namespace.urn("Building", 1)?;
    let stub = EntityFragment::for_entity(namespace.urn("Building", 1)?, "Building").build()?;
    let operation = MergeEntity::builder().id(&id).fragment(&stub).build();
    let document = UncheckedDocument::from_entity(&bare_building(namespace, 1)?, Lossless::Normalized)
        .set_member("@context", &context_member(vocabulary)?)
        .set_member("buildingDetails", &details.parse::<RawJson>()?);
    let request = content_type(broker.unchecked(&operation)?.document_as_is(&document), LD_JSON);
    let answer = broker.send_unchecked(request).await;
    verify_that!(&answer, answered(StatusCode::NO_CONTENT))?;
    expect_that!(answer?.body().len(), eq(0));
    Ok(())
}

/// Pins: Check that keys inside a Property object value can be merged and removed via NGSI-LD Null
/// Clause: 5.6.17, 6.5.3.4
/// Changed: the body is compared as one exact document, where the Hurl file checks its members one by one
#[gtest]
#[tokio::test]
async fn e056_09_01_remove_key_from_property_object_value() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &TERMS).await?;
    create_building(&broker, &vocabulary, &namespace).await?;
    merge_building(
        &broker,
        &vocabulary,
        &namespace,
        r#"{"type":"Property","value":{"deprecated":"urn:ngsi-ld:null"}}"#,
    )
    .await?;
    let client = broker.with_context(vocabulary.context())?.with_accept(NgsiLdMediaType::JsonLd);
    let id = namespace.urn("Building", 1)?;
    let body = retrieve_entity_json(&client, &id).await?;
    expect_that!(
        &body,
        matches_document(json!({
            "id": namespace.urn_text("Building", 1),
            "type": "Building",
            "buildingDetails": {
                "type": "Property",
                "value": {"category": "commercial", "floors": 5},
            },
            "@context": vocabulary.url().as_str(),
        }))
        .hiding("instanceId")
    );
    delete_entities(&broker, &[&id]).await;
    Ok(())
}

/// Pins: Check that keys inside a Property object value can be merged and removed via NGSI-LD Null
/// Clause: 5.6.17, 6.5.3.4
/// Changed: the body is compared as one exact document, where the Hurl file checks its members one by one
#[gtest]
#[tokio::test]
async fn e056_09_02_merge_keys_in_property_object_value() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &TERMS).await?;
    create_building(&broker, &vocabulary, &namespace).await?;
    merge_building(
        &broker,
        &vocabulary,
        &namespace,
        r#"{"type":"Property","value":{"category":"leisure","floors":"urn:ngsi-ld:null","deprecated":false}}"#,
    )
    .await?;
    let client = broker.with_context(vocabulary.context())?.with_accept(NgsiLdMediaType::JsonLd);
    let id = namespace.urn("Building", 1)?;
    let body = retrieve_entity_json(&client, &id).await?;
    expect_that!(
        &body,
        matches_document(json!({
            "id": namespace.urn_text("Building", 1),
            "type": "Building",
            "buildingDetails": {
                "type": "Property",
                "value": {"category": "leisure", "deprecated": false},
            },
            "@context": vocabulary.url().as_str(),
        }))
        .hiding("instanceId")
    );
    delete_entities(&broker, &[&id]).await;
    Ok(())
}
