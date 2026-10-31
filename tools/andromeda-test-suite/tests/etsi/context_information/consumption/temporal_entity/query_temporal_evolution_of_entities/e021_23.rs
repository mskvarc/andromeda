//! The temporal evolutions of Buildings are queried with `orderBy`. Only ordering by `id` is answered; every other
//! ordering is refused with `BadRequestData` (clause 5.7.4.4). The refused orderings are sent as written on the wire,
//! because the typed temporal query refuses them before sending.

use crate::{
    building_fixtures::{BUILDING_TERMS, create_named_buildings, delete_named_buildings, expect_building_order},
    unchecked_requests::refused_get,
};
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
        ordering,
        temporal_query::{self, TemporalProperty},
    },
};

/// The temporal query of the Buildings refused with `BadRequestData` for `order_by`, the `orderBy` value as written
/// on the wire.
async fn refused_order(broker: &BrokerClient, vocabulary: &Vocabulary, order_by: &str) -> Result<()> {
    let client = broker.with_context(vocabulary.context())?;
    refused_get(
        &client,
        &format!("temporal/entities?type=Building&timerel=after&timeAt=1970-01-01T00%3A00%3A00Z&orderBy={order_by}&timeproperty=createdAt"),
    )
    .await
}

/// Pins: Check that one can query entities with orderBy on an attribute
/// Clause: 5.7.4, 4.23
/// Expected: a temporal orderBy other than id is refused with `BadRequestData` (clause 5.7.4.4)
#[gtest]
#[tokio::test]
async fn e021_23_01_query_with_order_by_attribute() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &BUILDING_TERMS).await?;
    create_named_buildings(&broker, &vocabulary, &namespace).await?;
    refused_order(&broker, &vocabulary, "name").await?;
    delete_named_buildings(&broker, &namespace).await?;
    Ok(())
}

/// Pins: Check that one can query entities with orderBy on id
/// Clause: 5.7.4, 4.23
/// Expected: a temporal orderBy other than id is refused with `BadRequestData` (clause 5.7.4.4)
#[gtest]
#[tokio::test]
async fn e021_23_02_query_with_order_by_id() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &BUILDING_TERMS).await?;
    create_named_buildings(&broker, &vocabulary, &namespace).await?;
    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
        .with_responses()
        .temporal()
        .query(temporal_query::after("1970-01-01T00:00:00Z".parse()?).time_property(TemporalProperty::CreatedAt))
        .entity_type("Building")
        .ordering(ordering::by("id"))
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    expect_building_order(&body, &namespace, [1, 2, 3]);
    delete_named_buildings(&broker, &namespace).await?;
    Ok(())
}

/// Pins: Check that one can query entities with orderBy on createdAt
/// Clause: 5.7.4, 4.23
/// Expected: a temporal orderBy other than id is refused with `BadRequestData` (clause 5.7.4.4)
#[gtest]
#[tokio::test]
async fn e021_23_03_query_with_order_by_created_at() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &BUILDING_TERMS).await?;
    create_named_buildings(&broker, &vocabulary, &namespace).await?;
    refused_order(&broker, &vocabulary, "createdAt").await?;
    delete_named_buildings(&broker, &namespace).await?;
    Ok(())
}

/// Pins: Check that one can query entities with orderBy on an attribute createdAt
/// Clause: 5.7.4, 4.23
/// Expected: a temporal orderBy other than id is refused with `BadRequestData` (clause 5.7.4.4)
#[gtest]
#[tokio::test]
async fn e021_23_04_query_with_order_by_name_created_at() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &BUILDING_TERMS).await?;
    create_named_buildings(&broker, &vocabulary, &namespace).await?;
    refused_order(&broker, &vocabulary, "name.createdAt").await?;
    delete_named_buildings(&broker, &namespace).await?;
    Ok(())
}

/// Pins: Check that one can query entities with orderBy an attribute subproperty
/// Clause: 5.7.4, 4.23
/// Expected: a temporal orderBy other than id is refused with `BadRequestData` (clause 5.7.4.4)
#[gtest]
#[tokio::test]
async fn e021_23_05_query_with_order_by_sub_attribute() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &BUILDING_TERMS).await?;
    create_named_buildings(&broker, &vocabulary, &namespace).await?;
    refused_order(&broker, &vocabulary, "name.subProperty").await?;
    delete_named_buildings(&broker, &namespace).await?;
    Ok(())
}

/// Pins: Check that one can query entities with orderBy on a name ascending
/// Clause: 5.7.4, 4.23
/// Expected: a temporal orderBy other than id is refused with `BadRequestData` (clause 5.7.4.4)
#[gtest]
#[tokio::test]
async fn e021_23_06_query_with_order_by_ascending() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &BUILDING_TERMS).await?;
    create_named_buildings(&broker, &vocabulary, &namespace).await?;
    refused_order(&broker, &vocabulary, "name%3Basc").await?;
    delete_named_buildings(&broker, &namespace).await?;
    Ok(())
}

/// Pins: Check that one can query entities with orderBy on a name descending
/// Clause: 5.7.4, 4.23
/// Expected: a temporal orderBy other than id is refused with `BadRequestData` (clause 5.7.4.4)
#[gtest]
#[tokio::test]
async fn e021_23_07_query_with_order_by_descending() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &BUILDING_TERMS).await?;
    create_named_buildings(&broker, &vocabulary, &namespace).await?;
    refused_order(&broker, &vocabulary, "name%3Bdesc").await?;
    delete_named_buildings(&broker, &namespace).await?;
    Ok(())
}

/// Pins: Check that one can query entities with orderBy on multiple members
/// Clause: 5.7.4, 4.23
/// Expected: a temporal orderBy other than id is refused with `BadRequestData` (clause 5.7.4.4)
#[gtest]
#[tokio::test]
async fn e021_23_08_query_with_multiple_order_by() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &BUILDING_TERMS).await?;
    create_named_buildings(&broker, &vocabulary, &namespace).await?;
    refused_order(&broker, &vocabulary, "type%2Cname").await?;
    delete_named_buildings(&broker, &namespace).await?;
    Ok(())
}

/// Pins: Check that one can query entities with a complex orderBy
/// Clause: 5.7.4, 4.23
/// Expected: a temporal orderBy other than id is refused with `BadRequestData` (clause 5.7.4.4)
#[gtest]
#[tokio::test]
async fn e021_23_09_query_with_complex_order_by() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &BUILDING_TERMS).await?;
    create_named_buildings(&broker, &vocabulary, &namespace).await?;
    refused_order(&broker, &vocabulary, "type%3Basc%2Cname.subProperty%3Bdesc%2Cname%3Basc").await?;
    delete_named_buildings(&broker, &namespace).await?;
    Ok(())
}
