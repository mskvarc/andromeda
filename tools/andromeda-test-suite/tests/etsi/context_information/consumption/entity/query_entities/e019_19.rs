//! Buildings are queried with `orderBy` on an attribute, on `id`, on `createdAt`, on a sub-attribute or on a
//! sub-attribute's `createdAt`, ascending or descending, on several keys, and on a complex ordering (clauses 5.7.2 and
//! 4.23).

use crate::building_fixtures::{BUILDING_TERMS, create_named_buildings, delete_named_buildings, expect_building_order};
use andromeda_test_suite::{
    broker::{self, BrokerClient},
    namespace::Namespace,
    problem::answered,
    vocabulary::Vocabulary,
    wire::json_body,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    query::{
        ordering::{self, builders::EntityOrderingBuilder},
        q::attr,
    },
};
use serde_json::Value;

/// Queries the Buildings ordered by `ordering`, requires 200 and answers the body.
async fn query_ordered(broker: &BrokerClient, vocabulary: &Vocabulary, ordering: EntityOrderingBuilder) -> Result<Value> {
    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
        .with_responses()
        .entities()
        .query()
        .entity_type("Building")
        .ordering(ordering)
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    Ok(json_body(&received?)?)
}

/// Pins: Check that one can query entities with orderBy on an attribute
/// Clause: 5.7.2, 4.23
#[gtest]
#[tokio::test]
async fn e019_19_01_query_with_order_by_attribute() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &BUILDING_TERMS).await?;
    create_named_buildings(&broker, &vocabulary, &namespace).await?;
    let body = query_ordered(&broker, &vocabulary, ordering::by("name")).await?;
    expect_building_order(&body, &namespace, [1, 2, 3]);
    delete_named_buildings(&broker, &namespace).await?;
    Ok(())
}

/// Pins: Check that one can query entities with orderBy on id
/// Clause: 5.7.2, 4.23
#[gtest]
#[tokio::test]
async fn e019_19_02_query_with_order_by_id() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &BUILDING_TERMS).await?;
    create_named_buildings(&broker, &vocabulary, &namespace).await?;
    let body = query_ordered(&broker, &vocabulary, ordering::by("id")).await?;
    expect_building_order(&body, &namespace, [1, 2, 3]);
    delete_named_buildings(&broker, &namespace).await?;
    Ok(())
}

/// Pins: Check that one can query entities with orderBy on createdAt
/// Clause: 5.7.2, 4.23
#[gtest]
#[tokio::test]
async fn e019_19_03_query_with_order_by_created_at() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &BUILDING_TERMS).await?;
    create_named_buildings(&broker, &vocabulary, &namespace).await?;
    let body = query_ordered(&broker, &vocabulary, ordering::by("createdAt")).await?;
    expect_building_order(&body, &namespace, [2, 1, 3]);
    delete_named_buildings(&broker, &namespace).await?;
    Ok(())
}

/// Pins: Check that one can query entities with orderBy on an attribute createdAt
/// Clause: 5.7.2, 4.23
#[gtest]
#[tokio::test]
async fn e019_19_04_query_with_order_by_name_created_at() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &BUILDING_TERMS).await?;
    create_named_buildings(&broker, &vocabulary, &namespace).await?;
    let body = query_ordered(&broker, &vocabulary, ordering::by(attr("name").sub_attr("createdAt"))).await?;
    expect_building_order(&body, &namespace, [2, 1, 3]);
    delete_named_buildings(&broker, &namespace).await?;
    Ok(())
}

/// Pins: Check that one can query entities with orderBy an attribute subproperty
/// Clause: 5.7.2, 4.23
#[gtest]
#[tokio::test]
async fn e019_19_05_query_with_order_by_sub_attribute() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &BUILDING_TERMS).await?;
    create_named_buildings(&broker, &vocabulary, &namespace).await?;
    let body = query_ordered(&broker, &vocabulary, ordering::by(attr("name").sub_attr("subProperty"))).await?;
    expect_building_order(&body, &namespace, [1, 2, 3]);
    delete_named_buildings(&broker, &namespace).await?;
    Ok(())
}

/// Pins: Check that one can query entities with orderBy on a name ascending
/// Clause: 5.7.2, 4.23
#[gtest]
#[tokio::test]
async fn e019_19_06_query_with_order_by_ascending() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &BUILDING_TERMS).await?;
    create_named_buildings(&broker, &vocabulary, &namespace).await?;
    let body = query_ordered(&broker, &vocabulary, ordering::ascending("name")).await?;
    expect_building_order(&body, &namespace, [1, 2, 3]);
    delete_named_buildings(&broker, &namespace).await?;
    Ok(())
}

/// Pins: Check that one can query entities with orderBy on a name descending
/// Clause: 5.7.2, 4.23
#[gtest]
#[tokio::test]
async fn e019_19_07_query_with_order_by_descending() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &BUILDING_TERMS).await?;
    create_named_buildings(&broker, &vocabulary, &namespace).await?;
    let body = query_ordered(&broker, &vocabulary, ordering::descending("name")).await?;
    expect_building_order(&body, &namespace, [3, 2, 1]);
    delete_named_buildings(&broker, &namespace).await?;
    Ok(())
}

/// Pins: Check that one can query entities with orderBy on multiple members
/// Clause: 5.7.2, 4.23
#[gtest]
#[tokio::test]
async fn e019_19_08_query_with_multiple_order_by() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &BUILDING_TERMS).await?;
    create_named_buildings(&broker, &vocabulary, &namespace).await?;
    let body = query_ordered(&broker, &vocabulary, ordering::by("type").then("name")).await?;
    expect_building_order(&body, &namespace, [1, 2, 3]);
    delete_named_buildings(&broker, &namespace).await?;
    Ok(())
}

/// Pins: Check that one can query entities with a complex orderBy
/// Clause: 5.7.2, 4.23
#[gtest]
#[tokio::test]
async fn e019_19_09_query_with_complex_order_by() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &BUILDING_TERMS).await?;
    create_named_buildings(&broker, &vocabulary, &namespace).await?;
    let ordering = ordering::ascending("type")
        .then_descending(attr("name").sub_attr("subProperty"))
        .then_ascending("name");
    let body = query_ordered(&broker, &vocabulary, ordering).await?;
    expect_building_order(&body, &namespace, [3, 2, 1]);
    delete_named_buildings(&broker, &namespace).await?;
    Ok(())
}
