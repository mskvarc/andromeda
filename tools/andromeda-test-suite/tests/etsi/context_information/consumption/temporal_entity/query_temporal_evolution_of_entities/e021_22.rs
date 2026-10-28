//! ETSI `021_22`: the temporal evolutions of the Vehicles, the Bus and the Buildings are queried with `pick` and `omit`
//! beside `attrs`, or with a member named in both `pick` and `omit`. Each query is refused with `BadRequestData`
//! (clauses 5.7.4 and 4.2.1).

use crate::suite_keywords::{create_vehicle_pair_and_bus_in, delete_vehicle_pair_and_bus};
use andromeda_test_suite::{
    broker::{self, BrokerClient},
    namespace::Namespace,
    problem::problem,
    vocabulary::Vocabulary,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::ErrorType,
    query::{projection, temporal_query, type_selection},
};

/// The terms of the `@context` of the cases: the Building is named in the query, so it is defined in the context.
const TERMS: [&str; 5] = ["Vehicle", "speed", "fuelLevel", "Bus", "Building"];

/// Registers the terms of the cases, then creates Vehicles `021-22-A` and `021-22-B` and Bus `021-22-C` in their
/// context.
async fn create_setup(broker: &BrokerClient, namespace: &Namespace) -> Result<Vocabulary> {
    let vocabulary = Vocabulary::register(broker, namespace, &TERMS).await?;
    create_vehicle_pair_and_bus_in(broker, &vocabulary, namespace, "021-22", "021-22-C").await?;
    Ok(vocabulary)
}

/// Pins: Check that a `BadRequestDataException` is returned if an entity member is present in pick and omit
/// Clause: 5.7.4, 4.2.1
/// Defect: D-80: Andromeda admits a member named in both pick and omit instead of refusing it with `BadRequestData`;
/// GET temporal/entities?pick=speed&omit=speed answers 200 []
#[gtest]
#[tokio::test]
async fn e021_22_01_retrieve_with_same_entity_member_in_pick_and_omit() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_setup(&broker, &namespace).await?;

    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
        .with_responses()
        .temporal()
        .query_projected(temporal_query::after("1970-01-01T00:00:00Z".parse()?))
        .entity_type(type_selection::entity_type("Building").or("Vehicle").build()?)
        .pick(projection::attributes(["speed"]).build()?)
        .omit(projection::attributes(["speed"]).build()?)
        .send()
        .await;
    expect_that!(&received, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());

    delete_vehicle_pair_and_bus(&broker, &namespace, "021-22", "021-22-C").await?;
    Ok(())
}

/// Pins: Check that a `BadRequestDataException` is returned if pick and attrs query params are present
/// Clause: 5.7.4, 4.2.1
#[gtest]
#[tokio::test]
async fn e021_22_02_retrieve_with_pick_and_attrs() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_setup(&broker, &namespace).await?;

    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
        .with_responses()
        .temporal()
        .query_projected(temporal_query::after("1970-01-01T00:00:00Z".parse()?))
        .entity_type(type_selection::entity_type("Building").or("Vehicle").build()?)
        .attrs(vec!["fuelLevel".parse()?])
        .pick(projection::attributes(["speed"]).build()?)
        .send()
        .await;
    expect_that!(&received, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());

    delete_vehicle_pair_and_bus(&broker, &namespace, "021-22", "021-22-C").await?;
    Ok(())
}

/// Pins: Check that a `BadRequestDataException` is returned if omit and attrs query params are present
/// Clause: 5.7.4, 4.2.1
#[gtest]
#[tokio::test]
async fn e021_22_03_retrieve_with_omit_and_attrs() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_setup(&broker, &namespace).await?;

    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
        .with_responses()
        .temporal()
        .query(temporal_query::after("1970-01-01T00:00:00Z".parse()?))
        .entity_type(type_selection::entity_type("Building").or("Vehicle").build()?)
        .attrs(vec!["fuelLevel".parse()?])
        .omit(projection::attributes(["speed"]).build()?)
        .send()
        .await;
    expect_that!(&received, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());

    delete_vehicle_pair_and_bus(&broker, &namespace, "021-22", "021-22-C").await?;
    Ok(())
}
