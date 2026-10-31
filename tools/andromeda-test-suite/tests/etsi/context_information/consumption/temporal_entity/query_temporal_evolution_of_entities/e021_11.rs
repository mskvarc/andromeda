//! The temporal evolutions of the entities of the types a request names are queried with a limit on the number of
//! entities answered (clause 5.7.4). Every Vehicle of the setup is a member of the selected types, so the count of the
//! answer is the limit.

use crate::{
    temporal_operations::{create_temporal_entity, delete_temporal_entities},
    vehicle_evolution_payloads::vehicle_speed_fuel_level_on,
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
    query::{temporal_query, type_selection},
};
use serde_json::{Value, json};

/// Registers the terms of the cases and creates Vehicle `<ns>-1` observed on 2020-08-01, and Vehicles `<ns>-2` and
/// `<ns>-3` observed on 2020-09-01, in the vocabulary's context.
async fn create_vehicles(broker: &BrokerClient, namespace: &Namespace) -> Result<Vocabulary> {
    let vocabulary = Vocabulary::register(broker, namespace, &["Vehicle", "speed", "fuelLevel", "Bus"]).await?;
    create_temporal_entity(broker, &vocabulary, &vehicle_speed_fuel_level_on(namespace, 1, "2020-08-01")?).await?;
    create_temporal_entity(broker, &vocabulary, &vehicle_speed_fuel_level_on(namespace, 2, "2020-09-01")?).await?;
    create_temporal_entity(broker, &vocabulary, &vehicle_speed_fuel_level_on(namespace, 3, "2020-09-01")?).await?;
    Ok(vocabulary)
}

/// Queries the Bus and Vehicle entities after 2020-07-01T12:05:00Z, answering at most `limit` of them, and answers the
/// body.
async fn limited_query(broker: &BrokerClient, vocabulary: &Vocabulary, limit: u32) -> Result<Value> {
    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
        .with_responses()
        .temporal()
        .query(temporal_query::after("2020-07-01T12:05:00Z".parse()?))
        .entity_type(type_selection::entity_type("Bus").or("Vehicle"))
        .limit(limit)
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    Ok(json_body(&received?)?)
}

/// Whether `body` is an array of `count` entities, each of type `Vehicle`.
fn vehicles_only(body: &Value, count: usize) -> bool {
    body.as_array()
        .is_some_and(|items| items.len() == count && items.iter().all(|item| item.get("type") == Some(&json!("Vehicle"))))
}

/// Pins: Check that one can query the temporal evolution of entities with a limit to the number of entities to be
/// retrieved
/// Clause: 5.7.4
#[gtest]
#[tokio::test]
async fn e021_11_01_query_some_entities() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_vehicles(&broker, &namespace).await?;

    let body = limited_query(&broker, &vocabulary, 2).await?;
    expect_that!(body.as_array().map(Vec::len), some(eq(2)));
    expect_that!(vehicles_only(&body, 2), is_true());

    delete_temporal_entities(
        &broker,
        &[&namespace.urn("Vehicle", 1)?, &namespace.urn("Vehicle", 2)?, &namespace.urn("Vehicle", 3)?],
    )
    .await;
    Ok(())
}

/// Pins: Check that one can query the temporal evolution of entities with a limit to the number of entities to be
/// retrieved
/// Clause: 5.7.4
#[gtest]
#[tokio::test]
async fn e021_11_02_query_all_entities() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_vehicles(&broker, &namespace).await?;

    let body = limited_query(&broker, &vocabulary, 3).await?;
    expect_that!(body.as_array().map(Vec::len), some(eq(3)));
    expect_that!(vehicles_only(&body, 3), is_true());

    delete_temporal_entities(
        &broker,
        &[&namespace.urn("Vehicle", 1)?, &namespace.urn("Vehicle", 2)?, &namespace.urn("Vehicle", 3)?],
    )
    .await;
    Ok(())
}
