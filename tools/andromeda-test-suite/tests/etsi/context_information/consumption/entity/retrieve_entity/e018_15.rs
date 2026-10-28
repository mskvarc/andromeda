//! ETSI `018_15`: an entity is retrieved with the `format` and `options` query parameters naming each representation,
//! and with both together (clauses 5.7.1 and 6.3.7).

use crate::{
    suite_keywords::{create_entity, get_json},
    suite_payloads::building_simple_attributes,
};
use andromeda_test_suite::{
    broker::{self, BrokerClient},
    comparison::{ContextRule, matches_document},
    namespace::Namespace,
    teardown::AnyAnswer,
    vocabulary::Vocabulary,
};
use googletest::prelude::*;
use ngsi_ld::model::EntityId;
use serde_json::{Value, json};

/// The terms of the case's `@context`.
const TERMS: [&str; 5] = ["Building", "name", "subCategory", "airQualityLevel", "almostFull"];

/// The core `@context` a response names when its request carries none.
const CORE_CONTEXT: &str = "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld";

/// The keyValues representation of the case's Building: each property is its value, under its expanded name.
fn key_values(namespace: &Namespace) -> Result<Value> {
    Ok(json!({
        "id": namespace.urn_text("Building", 1),
        "type": namespace.expanded_term("Building")?,
        namespace.expanded_term("name")?: "Eiffel Tower",
        namespace.expanded_term("airQualityLevel")?: 4,
        namespace.expanded_term("almostFull")?: false,
        namespace.expanded_term("subCategory")?: "tourism",
        "@context": CORE_CONTEXT,
    }))
}

/// The normalized representation of the case's Building: each property is a Property with its value.
fn normalized(namespace: &Namespace) -> Result<Value> {
    Ok(json!({
        "id": namespace.urn_text("Building", 1),
        "type": namespace.expanded_term("Building")?,
        namespace.expanded_term("name")?: {"type": "Property", "value": "Eiffel Tower"},
        namespace.expanded_term("airQualityLevel")?: {
            "type": "Property",
            "value": 4,
            "unitCode": "C62",
            "observedAt": "2020-09-09T16:40:00.000Z",
        },
        namespace.expanded_term("almostFull")?: {"type": "Property", "value": false},
        namespace.expanded_term("subCategory")?: {"type": "Property", "value": "tourism"},
        "@context": CORE_CONTEXT,
    }))
}

/// Creates the case's Building in `vocabulary`'s context.
async fn create_building(broker: &BrokerClient, vocabulary: &Vocabulary, namespace: &Namespace) -> Result<EntityId> {
    let building = building_simple_attributes(namespace, 1)?;
    create_entity(broker, vocabulary, &building).await?;
    Ok(building.id)
}

/// Deletes `id` from the entity and temporal stores, as the cases' teardown does.
async fn delete(broker: &BrokerClient, id: &EntityId) {
    broker.entities().delete(id).send().await.any_answer();
    broker.temporal().delete(id).send().await.any_answer();
}

/// Pins: Check that one can retrieve with format query param having keyValues value
/// Clause: 5.7.1, 6.3.7
#[gtest]
#[tokio::test]
async fn e018_15_01_retrieve_with_key_values_format() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &TERMS).await?;
    let id = create_building(&broker, &vocabulary, &namespace).await?;
    let body = get_json(&broker, &format!("entities/{id}?format=keyValues")).await?;
    expect_that!(
        &body,
        matches_document(key_values(&namespace)?).context_rule(ContextRule::AnyCoreContextVersion)
    );
    delete(&broker, &id).await;
    Ok(())
}

/// Pins: Check that one can retrieve with format query param having simplified value
/// Clause: 5.7.1, 6.3.7
#[gtest]
#[tokio::test]
async fn e018_15_02_retrieve_with_simplified_format() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &TERMS).await?;
    let id = create_building(&broker, &vocabulary, &namespace).await?;
    let body = get_json(&broker, &format!("entities/{id}?format=simplified")).await?;
    expect_that!(
        &body,
        matches_document(key_values(&namespace)?).context_rule(ContextRule::AnyCoreContextVersion)
    );
    delete(&broker, &id).await;
    Ok(())
}

/// Pins: Check that one can retrieve with format query param having normalized value
/// Clause: 5.7.1, 6.3.7
#[gtest]
#[tokio::test]
async fn e018_15_03_retrieve_with_normalized_format() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &TERMS).await?;
    let id = create_building(&broker, &vocabulary, &namespace).await?;
    let body = get_json(&broker, &format!("entities/{id}?format=normalized")).await?;
    expect_that!(
        &body,
        matches_document(normalized(&namespace)?).context_rule(ContextRule::AnyCoreContextVersion)
    );
    delete(&broker, &id).await;
    Ok(())
}

/// Pins: Check that one can retrieve with options query param having keyValues value
/// Clause: 5.7.1, 6.3.7
#[gtest]
#[tokio::test]
async fn e018_15_04_retrieve_with_key_values_options() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &TERMS).await?;
    let id = create_building(&broker, &vocabulary, &namespace).await?;
    let body = get_json(&broker, &format!("entities/{id}?options=keyValues")).await?;
    expect_that!(
        &body,
        matches_document(key_values(&namespace)?).context_rule(ContextRule::AnyCoreContextVersion)
    );
    delete(&broker, &id).await;
    Ok(())
}

/// Pins: Check that one can retrieve with options query param having simplified value
/// Clause: 5.7.1, 6.3.7
#[gtest]
#[tokio::test]
async fn e018_15_05_retrieve_with_simplified_options() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &TERMS).await?;
    let id = create_building(&broker, &vocabulary, &namespace).await?;
    let body = get_json(&broker, &format!("entities/{id}?options=simplified")).await?;
    expect_that!(
        &body,
        matches_document(key_values(&namespace)?).context_rule(ContextRule::AnyCoreContextVersion)
    );
    delete(&broker, &id).await;
    Ok(())
}

/// Pins: Check that one can retrieve with options query param having normalized value
/// Clause: 5.7.1, 6.3.7
#[gtest]
#[tokio::test]
async fn e018_15_06_retrieve_with_normalized_options() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &TERMS).await?;
    let id = create_building(&broker, &vocabulary, &namespace).await?;
    let body = get_json(&broker, &format!("entities/{id}?options=normalized")).await?;
    expect_that!(
        &body,
        matches_document(normalized(&namespace)?).context_rule(ContextRule::AnyCoreContextVersion)
    );
    delete(&broker, &id).await;
    Ok(())
}

/// Pins: Check that one can retrieve with both format and options query params
/// Clause: 5.7.1, 6.3.7
#[gtest]
#[tokio::test]
async fn e018_15_07_retrieve_with_format_and_options() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &TERMS).await?;
    let id = create_building(&broker, &vocabulary, &namespace).await?;
    let body = get_json(&broker, &format!("entities/{id}?options=normalized&format=keyValues")).await?;
    expect_that!(
        &body,
        matches_document(key_values(&namespace)?).context_rule(ContextRule::AnyCoreContextVersion)
    );
    delete(&broker, &id).await;
    Ok(())
}
