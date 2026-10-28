//! ETSI `019_09`: several entities are selected with a `q` query parameter over their names, their air quality levels,
//! their `almostFull` Properties and their `locatedIn` Relationships, and the `NGSILD-Results-Count` header states how
//! many match (clause 5.7.2).

use crate::{
    suite_keywords::{create_entity, delete_entities, results_count},
    suite_payloads::{building_simple_attributes, pisa_tower, pisa_tower_sightseeing},
};
use andromeda_test_suite::{
    broker::{self, BrokerClient},
    namespace::Namespace,
    problem::answered,
    vocabulary::Vocabulary,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{headers::Accept, operation::ResultsCount},
    query::q::Query,
};

/// The terms of the `@context` the cases of this file register.
const TERMS: [&str; 6] = ["Building", "name", "subCategory", "airQualityLevel", "almostFull", "locatedIn"];

/// Creates the three Buildings of the cases, `urn:ngsi-ld:Building:<ns>-1` to `-3`, in `vocabulary`'s context.
async fn create_buildings(broker: &BrokerClient, vocabulary: &Vocabulary, namespace: &Namespace) -> Result<()> {
    create_entity(broker, vocabulary, &building_simple_attributes(namespace, 1)?).await?;
    create_entity(broker, vocabulary, &pisa_tower(namespace, 2)?).await?;
    create_entity(broker, vocabulary, &pisa_tower_sightseeing(namespace)?).await
}

/// Pins: Check that one can query several entities based on q query parameter
/// Clause: 5.7.2
#[gtest]
#[tokio::test]
async fn e019_09_01_query_on_name() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &TERMS).await?;
    create_buildings(&broker, &vocabulary, &namespace).await?;
    let q: Query = "name==\"Eiffel Tower\"".parse()?;
    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
        .with_responses()
        .entities()
        .query()
        .entity_type("Building")
        .q(q)
        .count(ResultsCount::Include)
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let received = received?;
    expect_that!(results_count(&received), some(eq("1")));
    delete_entities(
        &broker,
        &[&namespace.urn("Building", 1)?, &namespace.urn("Building", 2)?, &namespace.urn("Building", 3)?],
    )
    .await;
    Ok(())
}

/// Pins: Check that one can query several entities based on q query parameter
/// Clause: 5.7.2
#[gtest]
#[tokio::test]
async fn e019_09_02_query_on_name_and_almost_full_false() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &TERMS).await?;
    create_buildings(&broker, &vocabulary, &namespace).await?;
    let q: Query = "name==\"Pisa Tower\";almostFull==false".parse()?;
    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
        .with_responses()
        .entities()
        .query()
        .entity_type("Building")
        .q(q)
        .count(ResultsCount::Include)
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let received = received?;
    expect_that!(results_count(&received), some(eq("1")));
    delete_entities(
        &broker,
        &[&namespace.urn("Building", 1)?, &namespace.urn("Building", 2)?, &namespace.urn("Building", 3)?],
    )
    .await;
    Ok(())
}

/// Pins: Check that one can query several entities based on q query parameter
/// Clause: 5.7.2
#[gtest]
#[tokio::test]
async fn e019_09_03_query_on_name_and_almost_full_false() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &TERMS).await?;
    create_buildings(&broker, &vocabulary, &namespace).await?;
    let q: Query = "name==\"Pisa Tower\";almostFull==false".parse()?;
    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
        .with_responses()
        .entities()
        .query()
        .entity_type("Building")
        .q(q)
        .count(ResultsCount::Include)
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let received = received?;
    expect_that!(results_count(&received), some(eq("1")));
    delete_entities(
        &broker,
        &[&namespace.urn("Building", 1)?, &namespace.urn("Building", 2)?, &namespace.urn("Building", 3)?],
    )
    .await;
    Ok(())
}

/// Pins: Check that one can query several entities based on q query parameter
/// Clause: 5.7.2
#[gtest]
#[tokio::test]
async fn e019_09_04_query_on_name_or_almost_full_false() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &TERMS).await?;
    create_buildings(&broker, &vocabulary, &namespace).await?;
    let q: Query = "name==\"Pisa Tower\"|almostFull==false".parse()?;
    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
        .with_responses()
        .entities()
        .query()
        .entity_type("Building")
        .q(q)
        .count(ResultsCount::Include)
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let received = received?;
    expect_that!(results_count(&received), some(eq("3")));
    delete_entities(
        &broker,
        &[&namespace.urn("Building", 1)?, &namespace.urn("Building", 2)?, &namespace.urn("Building", 3)?],
    )
    .await;
    Ok(())
}

/// Pins: Check that one can query several entities based on q query parameter
/// Clause: 5.7.2
#[gtest]
#[tokio::test]
async fn e019_09_05_query_on_names_and_almost_full_false() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &TERMS).await?;
    create_buildings(&broker, &vocabulary, &namespace).await?;
    let q: Query = "(name==\"Pisa Tower\"|name==\"Eiffel Tower\");almostFull==false".parse()?;
    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
        .with_responses()
        .entities()
        .query()
        .entity_type("Building")
        .q(q)
        .count(ResultsCount::Include)
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let received = received?;
    expect_that!(results_count(&received), some(eq("2")));
    delete_entities(
        &broker,
        &[&namespace.urn("Building", 1)?, &namespace.urn("Building", 2)?, &namespace.urn("Building", 3)?],
    )
    .await;
    Ok(())
}

/// Pins: Check that one can query several entities based on q query parameter
/// Clause: 5.7.2
#[gtest]
#[tokio::test]
async fn e019_09_06_query_on_air_quality_level() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &TERMS).await?;
    create_buildings(&broker, &vocabulary, &namespace).await?;
    let q: Query = "airQualityLevel==4".parse()?;
    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
        .with_responses()
        .entities()
        .query()
        .entity_type("Building")
        .q(q)
        .count(ResultsCount::Include)
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let received = received?;
    expect_that!(results_count(&received), some(eq("1")));
    delete_entities(
        &broker,
        &[&namespace.urn("Building", 1)?, &namespace.urn("Building", 2)?, &namespace.urn("Building", 3)?],
    )
    .await;
    Ok(())
}

/// Pins: Check that one can query several entities based on q query parameter
/// Clause: 5.7.2
#[gtest]
#[tokio::test]
async fn e019_09_07_query_on_air_quality_level_range() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &TERMS).await?;
    create_buildings(&broker, &vocabulary, &namespace).await?;
    let q: Query = "airQualityLevel>=4;airQualityLevel<7".parse()?;
    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
        .with_responses()
        .entities()
        .query()
        .entity_type("Building")
        .q(q)
        .count(ResultsCount::Include)
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let received = received?;
    expect_that!(results_count(&received), some(eq("2")));
    delete_entities(
        &broker,
        &[&namespace.urn("Building", 1)?, &namespace.urn("Building", 2)?, &namespace.urn("Building", 3)?],
    )
    .await;
    Ok(())
}

/// Pins: Check that one can query several entities based on q query parameter
/// Clause: 5.7.2
#[gtest]
#[tokio::test]
async fn e019_09_08_query_on_located_in_existing() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &TERMS).await?;
    create_buildings(&broker, &vocabulary, &namespace).await?;
    let q: Query = format!("locatedIn=={}", namespace.urn_text("City", "Pisa")).parse()?;
    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
        .with_responses()
        .entities()
        .query()
        .entity_type("Building")
        .q(q)
        .count(ResultsCount::Include)
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let received = received?;
    expect_that!(results_count(&received), some(eq("1")));
    delete_entities(
        &broker,
        &[&namespace.urn("Building", 1)?, &namespace.urn("Building", 2)?, &namespace.urn("Building", 3)?],
    )
    .await;
    Ok(())
}

/// Pins: Check that one can query several entities based on q query parameter
/// Clause: 5.7.2
#[gtest]
#[tokio::test]
async fn e019_09_09_query_on_located_in_non_existing() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &TERMS).await?;
    create_buildings(&broker, &vocabulary, &namespace).await?;
    let q: Query = format!("locatedIn=={}", namespace.urn_text("City", "Paris")).parse()?;
    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
        .with_responses()
        .entities()
        .query()
        .entity_type("Building")
        .q(q)
        .count(ResultsCount::Include)
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let received = received?;
    expect_that!(results_count(&received), some(eq("0")));
    delete_entities(
        &broker,
        &[&namespace.urn("Building", 1)?, &namespace.urn("Building", 2)?, &namespace.urn("Building", 3)?],
    )
    .await;
    Ok(())
}

/// Pins: Check that one can query several entities based on q query parameter
/// Clause: 5.7.2
#[gtest]
#[tokio::test]
async fn e019_09_10_query_on_located_in_two_cities() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &TERMS).await?;
    create_buildings(&broker, &vocabulary, &namespace).await?;
    let q: Query = format!(
        "locatedIn=={}|locatedIn=={}",
        namespace.urn_text("City", "Pisa"),
        namespace.urn_text("City", "Paris")
    )
    .parse()?;
    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
        .with_responses()
        .entities()
        .query()
        .entity_type("Building")
        .q(q)
        .count(ResultsCount::Include)
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let received = received?;
    expect_that!(results_count(&received), some(eq("1")));
    delete_entities(
        &broker,
        &[&namespace.urn("Building", 1)?, &namespace.urn("Building", 2)?, &namespace.urn("Building", 3)?],
    )
    .await;
    Ok(())
}
