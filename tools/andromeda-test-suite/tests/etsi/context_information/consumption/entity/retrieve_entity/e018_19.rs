//! ETSI `018_19`: an entity is not retrieved when `pick` names no member the entity holds, or when `omit` names every
//! member it holds (clauses 5.7.1 and 4.21).

use crate::{
    suite_keywords::{create_entity, delete_entities},
    suite_payloads::{BUILDING_SIMPLE_TERMS, building_simple_attributes},
};
use andromeda_test_suite::{
    broker::{self, BrokerClient},
    namespace::Namespace,
    problem::problem,
    vocabulary::Vocabulary,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    model::{EntityId, ErrorType, NgsiLdMediaType},
    query::projection::{self, EntityMember},
};

/// The terms of the `@context` the unmatched pick case registers.
const UNKNOWN_TERMS: [&str; 6] = ["Building", "name", "subCategory", "airQualityLevel", "almostFull", "unknown"];

/// Creates the case's Building in a `@context` of `terms`, and answers the context.
async fn created_building(broker: &BrokerClient, namespace: &Namespace, terms: &[&str]) -> Result<Vocabulary> {
    let vocabulary = Vocabulary::register(broker, namespace, terms).await?;
    create_entity(broker, &vocabulary, &building_simple_attributes(namespace, 1)?).await?;
    Ok(vocabulary)
}

/// Pins: Check that a `ResourceNotFound` error is returned if pick does not match any entity member
/// Clause: 5.7.1, 4.21
#[gtest]
#[tokio::test]
async fn e018_19_01_retrieve_with_unmatched_pick() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = created_building(&broker, &namespace, &UNKNOWN_TERMS).await?;
    let id: EntityId = namespace.urn("Building", 1)?;
    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(NgsiLdMediaType::Json)
        .with_responses()
        .entities()
        .retrieve_projected(&id)
        .pick(projection::attributes(["unknown"]).build()?)
        .send()
        .await;
    expect_that!(&received, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound).with_title());
    delete_entities(&broker, &[&id]).await;
    Ok(())
}

/// Pins: Check that a `ResourceNotFound` error is returned if omit contains all entity members
/// Clause: 5.7.1, 4.21
#[gtest]
#[tokio::test]
async fn e018_19_02_retrieve_with_full_excluding_omit() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = created_building(&broker, &namespace, &BUILDING_SIMPLE_TERMS).await?;
    let id: EntityId = namespace.urn("Building", 1)?;
    let omit = projection::member(EntityMember::Id)
        .member(EntityMember::Type)
        .attribute("name")
        .attribute("subCategory")
        .attribute("airQualityLevel")
        .attribute("almostFull")
        .build()?;
    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(NgsiLdMediaType::Json)
        .with_responses()
        .entities()
        .retrieve_projected(&id)
        .omit(omit)
        .send()
        .await;
    expect_that!(&received, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound).with_title());
    delete_entities(&broker, &[&id]).await;
    Ok(())
}
