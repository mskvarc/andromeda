//! ETSI `024_02`: a specified entity type is retrieved in its detailed representation, named by its compacted or by its
//! expanded name (clause 5.7.7).

use crate::{suite_keywords::create_entity_with_link, suite_payloads::building_simple_attributes};
use andromeda_test_suite::{
    broker::{self, BrokerClient},
    comparison::matches_document,
    namespace::Namespace,
    problem::answered,
    teardown::AnyAnswer,
    vocabulary::Vocabulary,
    wire::json_body,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::model::{EntityId, EntityTypeName};
use serde_json::{Value, json};

/// The terms of the case's `@context`.
const TERMS: [&str; 5] = ["Building", "name", "subCategory", "airQualityLevel", "almostFull"];

/// The detailed representation of the Building type the cases read, with its two entities and four attributes.
fn building_type_detail(namespace: &Namespace) -> Result<Value> {
    Ok(json!({
        "id": namespace.expanded_term("Building")?,
        "type": "EntityTypeInfo",
        "typeName": "Building",
        "entityCount": 2,
        "attributeDetails": [
            attribute_detail(namespace, "name")?,
            attribute_detail(namespace, "subCategory")?,
            attribute_detail(namespace, "airQualityLevel")?,
            attribute_detail(namespace, "almostFull")?,
        ],
    }))
}

/// The detail of the attribute `name` of the Building type, as the answer writes it.
fn attribute_detail(namespace: &Namespace, name: &str) -> Result<Value> {
    Ok(json!({
        "id": namespace.expanded_term(name)?,
        "type": "Attribute",
        "attributeName": name,
        "attributeTypes": ["Property"],
    }))
}

/// Creates the two Buildings of the cases in `vocabulary`'s context, with the Link header naming it, and returns their
/// identifiers.
async fn create_buildings(broker: &BrokerClient, vocabulary: &Vocabulary, namespace: &Namespace) -> Result<[EntityId; 2]> {
    let first = building_simple_attributes(namespace, 1)?;
    let second = building_simple_attributes(namespace, 2)?;
    create_entity_with_link(broker, vocabulary, &first).await?;
    create_entity_with_link(broker, vocabulary, &second).await?;
    Ok([first.id, second.id])
}

/// Pins: Check that one can retrieve a detailed representation of a specified NGSI-LD entity type
/// Clause: 5.7.7
#[gtest]
#[tokio::test]
async fn e024_02_01_with_compacted_type() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &TERMS).await?;
    let buildings = create_buildings(&broker, &vocabulary, &namespace).await?;
    let building_type: EntityTypeName = "Building".parse()?;
    let received = broker
        .with_context(vocabulary.context())?
        .with_responses()
        .entity_types()
        .retrieve(&building_type)
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    expect_that!(&body, matches_document(building_type_detail(&namespace)?));
    for id in &buildings {
        broker.entities().delete(id).send().await.any_answer();
        broker.temporal().delete(id).send().await.any_answer();
    }
    Ok(())
}

/// Pins: Check that one can retrieve a detailed representation of a specified NGSI-LD entity type
/// Clause: 5.7.7
#[gtest]
#[tokio::test]
async fn e024_02_02_with_expanded_type() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &TERMS).await?;
    let buildings = create_buildings(&broker, &vocabulary, &namespace).await?;
    let building_type: EntityTypeName = namespace.expanded_term("Building")?.parse()?;
    let received = broker
        .with_context(vocabulary.context())?
        .with_responses()
        .entity_types()
        .retrieve(&building_type)
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    expect_that!(&body, matches_document(building_type_detail(&namespace)?));
    for id in &buildings {
        broker.entities().delete(id).send().await.any_answer();
        broker.temporal().delete(id).send().await.any_answer();
    }
    Ok(())
}
