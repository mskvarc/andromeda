//! Keys inside the object value of a Property are removed, and replaced, by a Merge Entity whose value holds the
//! NGSI-LD Null for them (clauses 5.6.17 and 6.5.3.4).

use crate::entity_operations::{create_entity, delete_entities, retrieve_entity_json};
use andromeda_test_suite::{
    broker::{self, BrokerClient},
    comparison::matches_document,
    namespace::Namespace,
    problem::answered,
    vocabulary::Vocabulary,
};
use googletest::prelude::*;
use http::StatusCode;
use indexmap::IndexMap;
use ngsi_ld::{
    http::ContextDelivery,
    model::{Attribute, Entity, EntityFragment, Input, NgsiLdMediaType, Property, Value, ValuePatch},
};
use serde_json::json;

/// The terms of the `@context` the cases register.
const TERMS: [&str; 2] = ["Building", "buildingDetails"];

/// Creates the Building `urn:ngsi-ld:Building:<ns>-1` whose `buildingDetails` is an object holding `category`
/// `commercial`, `floors` 5 and `deprecated` true. Requires 201.
async fn create_building(broker: &BrokerClient, vocabulary: &Vocabulary, namespace: &Namespace) -> Result<()> {
    let details = Value::Object(IndexMap::from([
        ("category".to_owned(), Value::from("commercial")),
        ("floors".to_owned(), Value::from(5)),
        ("deprecated".to_owned(), Value::from(true)),
    ]));
    let building: Entity<Input> = Entity::builder()
        .id(namespace.urn("Building", 1)?)
        .entity_type("Building")
        .property("buildingDetails", details)
        .build()?;
    create_entity(broker, vocabulary, &building).await
}

/// Merges `buildingDetails` patched by `details` into the Building `urn:ngsi-ld:Building:<ns>-1`, the vocabulary in
/// the body. Requires 204 and an empty body.
async fn merge_building(broker: &BrokerClient, vocabulary: &Vocabulary, namespace: &Namespace, details: ValuePatch) -> Result<()> {
    let id = namespace.urn("Building", 1)?;
    let fragment = EntityFragment::for_entity(id.clone(), "Building")
        .patch("buildingDetails", Attribute::from(Property::merge_builder(details).build()?))
        .build()?;
    let client = broker.with_context(vocabulary.context())?.with_context_delivery(ContextDelivery::LdJsonBody);
    let answer = client.with_responses().entities().merge(&id, &fragment).send().await;
    verify_that!(&answer, answered(StatusCode::NO_CONTENT))?;
    expect_that!(answer?.body.len(), eq(0));
    Ok(())
}

/// Pins: Check that keys inside a Property object value can be merged and removed via NGSI-LD Null
/// Clause: 5.6.17, 6.5.3.4
#[gtest]
#[tokio::test]
async fn e056_09_01_remove_key_from_property_object_value() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &TERMS).await?;
    create_building(&broker, &vocabulary, &namespace).await?;
    merge_building(&broker, &vocabulary, &namespace, ValuePatch::new().delete("deprecated")).await?;
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
        ValuePatch::new().set("category", "leisure").delete("floors").set("deprecated", false),
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
