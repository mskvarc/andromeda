//! An entity is retrieved with the `format` and `options` query parameters naming each representation, and with both
//! together (clauses 5.7.1 and 6.3.7).

use crate::{building_payloads::building_simple_attributes, entity_operations::create_entity, unchecked_requests::json_answer};
use andromeda_test_suite::{
    broker::{self, BrokerClient},
    comparison::{ContextRule, matches_document},
    namespace::Namespace,
    teardown::AnyAnswer,
    vocabulary::Vocabulary,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{
        headers::Accept,
        operation::{Operation, RepresentationSpelling, RepresentationToken, RetrieveEntity, Simplified},
    },
    model::{CORE_CONTEXT, EntityId, Lossless, NgsiLdMediaType},
};
use serde_json::{Value, json};

/// The terms of the case's `@context`.
const TERMS: [&str; 5] = ["Building", "name", "subCategory", "airQualityLevel", "almostFull"];

/// The keyValues representation of the case's Building: each property is its value, under its expanded name.
fn key_values(namespace: &Namespace) -> Result<Value> {
    Ok(json!({
        "id": namespace.urn_text("Building", 1),
        "type": namespace.expanded_term("Building")?,
        namespace.expanded_term("name")?: "Eiffel Tower",
        namespace.expanded_term("airQualityLevel")?: 4,
        namespace.expanded_term("almostFull")?: false,
        namespace.expanded_term("subCategory")?: "tourism",
        "@context": CORE_CONTEXT.as_str(),
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
        "@context": CORE_CONTEXT.as_str(),
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

/// The answer to `retrieve`, sent with `Accept: application/ld+json` and no `@context`: requires 200 and answers the
/// body as JSON, which the cases compare whole with the document they expect, `@context` and expanded names included.
async fn retrieved_json<O: Operation + Sync>(broker: &BrokerClient, retrieve: &O) -> Result<Value> {
    json_answer(broker, broker.with_accept(Accept::new(NgsiLdMediaType::JsonLd)).unchecked(retrieve)?).await
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
    let body = retrieved_json(
        &broker,
        &RetrieveEntity::builder()
            .id(&id)
            .format(Simplified)
            .spelling(RepresentationSpelling::default().key_values())
            .build(),
    )
    .await?;
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
    let body = retrieved_json(
        &broker,
        &RetrieveEntity::builder()
            .id(&id)
            .format(Simplified)
            .spelling(RepresentationSpelling::default())
            .build(),
    )
    .await?;
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
    let body = retrieved_json(
        &broker,
        &RetrieveEntity::builder()
            .id(&id)
            .format(Lossless::Normalized)
            .spelling(RepresentationSpelling::default().explicit_normalized())
            .build(),
    )
    .await?;
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
    let body = retrieved_json(
        &broker,
        &RetrieveEntity::builder()
            .id(&id)
            .format(Simplified)
            .spelling(RepresentationSpelling::default().in_options().key_values())
            .build(),
    )
    .await?;
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
    let body = retrieved_json(
        &broker,
        &RetrieveEntity::builder()
            .id(&id)
            .format(Simplified)
            .spelling(RepresentationSpelling::default().in_options())
            .build(),
    )
    .await?;
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
    let body = retrieved_json(
        &broker,
        &RetrieveEntity::builder()
            .id(&id)
            .format(Lossless::Normalized)
            .spelling(RepresentationSpelling::default().in_options().explicit_normalized())
            .build(),
    )
    .await?;
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
    let body = retrieved_json(
        &broker,
        &RetrieveEntity::builder()
            .id(&id)
            .format(Simplified)
            .spelling(
                RepresentationSpelling::default()
                    .key_values()
                    .format_over_options(RepresentationToken::Normalized),
            )
            .build(),
    )
    .await?;
    expect_that!(
        &body,
        matches_document(key_values(&namespace)?).context_rule(ContextRule::AnyCoreContextVersion)
    );
    delete(&broker, &id).await;
    Ok(())
}
