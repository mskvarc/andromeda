//! An entity is not retrieved when its identifier, or an attribute asked for, is unknown to the system (clause 5.7.1).

use crate::{building_payloads::building_simple_attributes, entity_operations::create_entity};
use andromeda_test_suite::{broker, namespace::Namespace, problem::problem, teardown::AnyAnswer, vocabulary::Vocabulary};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::model::{ErrorType, NgsiLdMediaType};

/// The terms of the case's `@context`.
const TERMS: [&str; 5] = ["Building", "name", "subCategory", "airQualityLevel", "almostFull"];

/// Pins: Check that one cannot get an entity if the entity id is not known to the system
/// Clause: 5.7.1
/// Vocabulary: none (the test references no @context)
#[gtest]
#[tokio::test]
async fn e018_03_01_get_an_entity_if_the_entity_id_is_not_known_to_the_system() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let answer = broker
        .with_accept(NgsiLdMediaType::JsonLd)
        .with_responses()
        .entities()
        .retrieve(&namespace.urn("Building", 1)?)
        .send()
        .await;
    verify_that!(&answer, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound).with_title())
}

/// Pins: Check that one cannot get an entity if an attribute is not known to the system
/// Clause: 5.7.1
#[gtest]
#[tokio::test]
async fn e018_03_02_get_an_entity_if_an_attribute_is_not_known_to_the_system() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &TERMS).await?;
    let building = building_simple_attributes(&namespace, 1)?;
    create_entity(&broker, &vocabulary, &building).await?;
    let answer = broker
        .with_responses()
        .entities()
        .retrieve(&building.id)
        .attrs(vec![namespace.attribute("property_not_found")?])
        .send()
        .await;
    expect_that!(&answer, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound).with_title());
    broker.entities().delete(&building.id).send().await.any_answer();
    broker.temporal().delete(&building.id).send().await.any_answer();
    Ok(())
}
