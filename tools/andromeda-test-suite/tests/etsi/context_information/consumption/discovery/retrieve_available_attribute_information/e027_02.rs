//! ETSI `027_02`: an attribute named with its short name or with its expanded name is retrieved in its detailed
//! representation (clause 5.7.10).

use crate::{suite_keywords::create_entity_with_link, suite_payloads::building_simple_attributes};
use andromeda_test_suite::{
    broker,
    comparison::matches_document,
    namespace::Namespace,
    problem::answered,
    teardown::AnyAnswer,
    vocabulary::Vocabulary,
    wire::json_body,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::model::AttributeName;
use serde_json::{Value, json};

/// The terms of the case's `@context`.
const TERMS: [&str; 5] = ["Building", "name", "subCategory", "airQualityLevel", "almostFull"];

/// The detailed representation of `airQualityLevel` in the test's vocabulary, with the one Building that holds it.
fn air_quality_level_detail(namespace: &Namespace) -> Result<Value> {
    Ok(json!({
        "id": namespace.expanded_term("airQualityLevel")?,
        "type": "Attribute",
        "attributeName": "airQualityLevel",
        "attributeCount": 1,
        "attributeTypes": ["Property"],
        "typeNames": ["Building"],
    }))
}

/// Pins: Check that one can retrieve a list with a detailed representation of NGSI-LD attributes
/// Clause: 5.7.10
#[gtest]
#[tokio::test]
async fn e027_02_01_with_compacted_attribute_name() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &TERMS).await?;
    let building = building_simple_attributes(&namespace, 1)?;
    create_entity_with_link(&broker, &vocabulary, &building).await?;
    let attribute: AttributeName = "airQualityLevel".parse()?;
    let received = broker
        .with_context(vocabulary.context())?
        .with_responses()
        .attributes()
        .retrieve(&attribute)
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    expect_that!(&body, matches_document(air_quality_level_detail(&namespace)?));
    broker.entities().delete(&building.id).send().await.any_answer();
    broker.temporal().delete(&building.id).send().await.any_answer();
    Ok(())
}

/// Pins: Check that one can retrieve a list with a detailed representation of NGSI-LD attributes
/// Clause: 5.7.10
#[gtest]
#[tokio::test]
async fn e027_02_02_with_expanded_attribute_name() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &TERMS).await?;
    let building = building_simple_attributes(&namespace, 1)?;
    create_entity_with_link(&broker, &vocabulary, &building).await?;
    let attribute: AttributeName = namespace.expanded_term("airQualityLevel")?.parse()?;
    let received = broker
        .with_context(vocabulary.context())?
        .with_responses()
        .attributes()
        .retrieve(&attribute)
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    expect_that!(&body, matches_document(air_quality_level_detail(&namespace)?));
    broker.entities().delete(&building.id).send().await.any_answer();
    broker.temporal().delete(&building.id).send().await.any_answer();
    Ok(())
}
