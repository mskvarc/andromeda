//! ETSI `018_06`: the JSON-LD `@context` of a response comes from the `Link` header the request carries, and the
//! default one applies when the request carries none (clause 6.3.5).

use crate::{
    suite_keywords::{OBSERVED_AT_WHOLE_SECOND, create_entity_with_link, observed_at},
    suite_payloads::{BUILDING_SIMPLE_TERMS, building_simple_attributes},
};
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
use ngsi_ld::model::NgsiLdMediaType;
use serde_json::{Map, json};

/// Pins: Check that the JSON-LD @context is obtained from a Link header if present and that the default JSON-LD
/// @context is used if not present
/// Clause: 6.3.5
#[gtest]
#[tokio::test]
async fn e018_06_01_empty_json_ld_context() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &BUILDING_SIMPLE_TERMS).await?;
    let building = building_simple_attributes(&namespace, 1)?;
    create_entity_with_link(&broker, &vocabulary, &building).await?;
    let received = broker
        .with_accept(NgsiLdMediaType::Json)
        .with_responses()
        .entities()
        .retrieve(&building.id)
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    let air_quality_level = namespace.expanded_term("airQualityLevel")?;
    let expected = json!({
        "id": building.id.to_string(),
        "type": namespace.expanded_term("Building")?,
        namespace.expanded_term("name")?: {"type": "Property", "value": "Eiffel Tower"},
        air_quality_level.as_str(): {"type": "Property", "value": 4, "unitCode": "C62", "observedAt": "2020-09-09T16:40:00.000Z"},
        namespace.expanded_term("almostFull")?: {"type": "Property", "value": false},
        namespace.expanded_term("subCategory")?: {"type": "Property", "value": "tourism"},
    });
    expect_that!(&body, matches_document(expected).hiding("instanceId"));
    expect_that!(body.as_object().map(Map::len), some(eq(6)));
    expect_that!(observed_at(&body, &air_quality_level), some(matches_regex(OBSERVED_AT_WHOLE_SECOND)));
    broker.entities().delete(&building.id).send().await.any_answer();
    broker.temporal().delete(&building.id).send().await.any_answer();
    Ok(())
}

/// Pins: Check that the JSON-LD @context is obtained from a Link header if present and that the default JSON-LD
/// @context is used if not present
/// Clause: 6.3.5
#[gtest]
#[tokio::test]
async fn e018_06_02_creation_time_json_ld_context() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &BUILDING_SIMPLE_TERMS).await?;
    let building = building_simple_attributes(&namespace, 1)?;
    create_entity_with_link(&broker, &vocabulary, &building).await?;
    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(NgsiLdMediaType::Json)
        .with_responses()
        .entities()
        .retrieve(&building.id)
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    let expected = json!({
        "id": building.id.to_string(),
        "type": "Building",
        "name": {"type": "Property", "value": "Eiffel Tower"},
        "subCategory": {"type": "Property", "value": "tourism"},
        "airQualityLevel": {"type": "Property", "value": 4, "unitCode": "C62", "observedAt": "2020-09-09T16:40:00.000Z"},
        "almostFull": {"type": "Property", "value": false},
    });
    expect_that!(&body, matches_document(expected).hiding("instanceId"));
    expect_that!(body.as_object().map(Map::len), some(eq(6)));
    expect_that!(observed_at(&body, "airQualityLevel"), some(matches_regex(OBSERVED_AT_WHOLE_SECOND)));
    broker.entities().delete(&building.id).send().await.any_answer();
    broker.temporal().delete(&building.id).send().await.any_answer();
    Ok(())
}
