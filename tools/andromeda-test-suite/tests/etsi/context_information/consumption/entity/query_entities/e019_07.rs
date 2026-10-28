//! ETSI `019_07`: with `count=true` the `NGSILD-Results-Count` header states the total number of matching entities
//! (clauses 5.7.2 and 6.3.13).

use crate::{
    suite_keywords::{create_entity, delete_entities, results_count},
    suite_payloads::{BUILDING_SIMPLE_TERMS, building_simple_attributes, pisa_tower},
};
use andromeda_test_suite::{broker, namespace::Namespace, problem::answered, vocabulary::Vocabulary};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{headers::Accept, operation::ResultsCount},
    query::q::Query,
};

/// Creates the two Buildings of the count cases, `urn:ngsi-ld:Building:<ns>-1` and `urn:ngsi-ld:Building:<ns>-2`.
async fn create_pair(broker: &broker::BrokerClient, vocabulary: &Vocabulary, namespace: &Namespace) -> Result<()> {
    create_entity(broker, vocabulary, &building_simple_attributes(namespace, 1)?).await?;
    create_entity(broker, vocabulary, &pisa_tower(namespace, 2)?).await
}

/// Pins: If the count parameter is set to true the special HTTP header NGSILD-Results-Count is set in the response and it
/// shall contain the total number of matching results.
/// Clause: 5.7.2, 6.3.13
#[gtest]
#[tokio::test]
async fn e019_07_01_count_with_only_the_entity_type() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &BUILDING_SIMPLE_TERMS).await?;
    create_pair(&broker, &vocabulary, &namespace).await?;
    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
        .with_responses()
        .entities()
        .query()
        .entity_type("Building")
        .count(ResultsCount::Include)
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let received = received?;
    expect_that!(results_count(&received), some(eq("2")));
    delete_entities(&broker, &[&namespace.urn("Building", 1)?, &namespace.urn("Building", 2)?]).await;
    Ok(())
}

/// Pins: If the count parameter is set to true the special HTTP header NGSILD-Results-Count is set in the response and it
/// shall contain the total number of matching results.
/// Clause: 5.7.2, 6.3.13
#[gtest]
#[tokio::test]
async fn e019_07_02_count_with_a_q_parameter() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &BUILDING_SIMPLE_TERMS).await?;
    create_pair(&broker, &vocabulary, &namespace).await?;
    let q: Query = "airQualityLevel==6".parse()?;
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
    delete_entities(&broker, &[&namespace.urn("Building", 1)?, &namespace.urn("Building", 2)?]).await;
    Ok(())
}
