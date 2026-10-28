//! ETSI `018_18`: an entity is not retrieved when `pick` and `omit` name the same member, when `attrs` is sent beside
//! `pick` or `omit`, or when the projection is malformed (clauses 5.7.1 and 4.21).

use crate::{
    suite_keywords::{create_entity, delete_entities, refused},
    suite_payloads::{BUILDING_SIMPLE_TERMS, building_simple_attributes},
};
use andromeda_test_suite::{
    broker::{self, BrokerClient},
    namespace::Namespace,
    problem::problem,
    vocabulary::Vocabulary,
    wire::request_to,
};
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::ACCEPT};
use ngsi_ld::{
    model::{EntityId, ErrorType, NgsiLdMediaType},
    query::projection,
};

/// The terms of the `@context` of the cases that name `locatedAt` in the query.
const LOCATED_AT_TERMS: [&str; 6] = ["Building", "name", "subCategory", "airQualityLevel", "almostFull", "locatedAt"];

/// The terms of the `@context` of the cases that name `category` in the query.
const CATEGORY_TERMS: [&str; 6] = ["Building", "name", "subCategory", "airQualityLevel", "almostFull", "category"];

/// Creates the case's Building in a `@context` of `terms`, and answers the context.
async fn created_building(broker: &BrokerClient, namespace: &Namespace, terms: &[&str]) -> Result<Vocabulary> {
    let vocabulary = Vocabulary::register(broker, namespace, terms).await?;
    create_entity(broker, &vocabulary, &building_simple_attributes(namespace, 1)?).await?;
    Ok(vocabulary)
}

/// Sends `GET entities/<id>?<query>` with the query written as the case writes it, as `application/json` in the
/// context `client` carries, and requires 400 with the problem type `BadRequestData` and a title.
async fn refused_query(client: &BrokerClient, id: &EntityId, query: &str) -> Result<()> {
    let request = request_to(client, Method::GET, &format!("entities/{id}?{query}"))?.append_header(ACCEPT, HeaderValue::from_static("application/json"));
    refused(client, request, ErrorType::BadRequestData).await
}

/// Pins: Check that a `BadRequestDataException` is returned if an entity member is present in pick and omit
/// Clause: 5.7.1, 4.21
/// Defect: D-80: Andromeda admits a member named in both pick and omit instead of refusing it with `BadRequestData`; GET entities/{id}?pick=name&omit=name answers 404 `ResourceNotFound`
#[gtest]
#[tokio::test]
async fn e018_18_01_retrieve_with_same_entity_member_in_pick_and_omit() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = created_building(&broker, &namespace, &BUILDING_SIMPLE_TERMS).await?;
    let id = namespace.urn("Building", 1)?;
    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(NgsiLdMediaType::Json)
        .with_responses()
        .entities()
        .retrieve_projected(&id)
        .pick(projection::attributes(["name"]).build()?)
        .omit(projection::attributes(["name"]).build()?)
        .send()
        .await;
    expect_that!(&received, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    delete_entities(&broker, &[&id]).await;
    Ok(())
}

/// Pins: Check that a `BadRequestDataException` is returned if pick and attrs query params are present
/// Clause: 5.7.1, 4.21
#[gtest]
#[tokio::test]
async fn e018_18_02_retrieve_with_pick_and_attrs() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = created_building(&broker, &namespace, &CATEGORY_TERMS).await?;
    let id = namespace.urn("Building", 1)?;
    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(NgsiLdMediaType::Json)
        .with_responses()
        .entities()
        .retrieve_projected(&id)
        .attrs(vec!["category".parse()?])
        .pick(projection::attributes(["name"]).build()?)
        .send()
        .await;
    expect_that!(&received, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    delete_entities(&broker, &[&id]).await;
    Ok(())
}

/// Pins: Check that a `BadRequestDataException` is returned if omit and attrs query params are present
/// Clause: 5.7.1, 4.21
#[gtest]
#[tokio::test]
async fn e018_18_03_retrieve_with_omit_and_attrs() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = created_building(&broker, &namespace, &CATEGORY_TERMS).await?;
    let id = namespace.urn("Building", 1)?;
    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(NgsiLdMediaType::Json)
        .with_responses()
        .entities()
        .retrieve_projected(&id)
        .attrs(vec!["category".parse()?])
        .omit(projection::attributes(["name"]).build()?)
        .send()
        .await;
    expect_that!(&received, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    delete_entities(&broker, &[&id]).await;
    Ok(())
}

/// Pins: Check that a `BadRequestDataException` is returned if an invalid character is present
/// Clause: 5.7.1, 4.21
#[gtest]
#[tokio::test]
async fn e018_18_04_retrieve_with_invalid_character() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = created_building(&broker, &namespace, &BUILDING_SIMPLE_TERMS).await?;
    let id = namespace.urn("Building", 1)?;
    refused_query(&broker.with_context(vocabulary.context())?, &id, "pick=id%3Bname").await?;
    delete_entities(&broker, &[&id]).await;
    Ok(())
}

/// Pins: Check that a `BadRequestDataException` is returned if a brace is not closed
/// Clause: 5.7.1, 4.21
#[gtest]
#[tokio::test]
async fn e018_18_05_retrieve_with_unclosed_brace() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = created_building(&broker, &namespace, &LOCATED_AT_TERMS).await?;
    let id = namespace.urn("Building", 1)?;
    refused_query(&broker.with_context(vocabulary.context())?, &id, "pick=id%2ClocatedAt%7Bname").await?;
    delete_entities(&broker, &[&id]).await;
    Ok(())
}

/// Pins: Check that a `BadRequestDataException` is returned if a double brace is present
/// Clause: 5.7.1, 4.21
#[gtest]
#[tokio::test]
async fn e018_18_06_retrieve_with_double_braces() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = created_building(&broker, &namespace, &LOCATED_AT_TERMS).await?;
    let id = namespace.urn("Building", 1)?;
    refused_query(&broker.with_context(vocabulary.context())?, &id, "pick=id%2ClocatedAt%7B%7Bname%7D").await?;
    delete_entities(&broker, &[&id]).await;
    Ok(())
}

/// Pins: Check that a `BadRequestDataException` is returned if consecutive separators are present
/// Clause: 5.7.1, 4.21
#[gtest]
#[tokio::test]
async fn e018_18_07_retrieve_with_consecutive_separators() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = created_building(&broker, &namespace, &BUILDING_SIMPLE_TERMS).await?;
    let id = namespace.urn("Building", 1)?;
    refused_query(&broker.with_context(vocabulary.context())?, &id, "pick=id%2C%2Cname").await?;
    delete_entities(&broker, &[&id]).await;
    Ok(())
}

/// Pins: Check that a `BadRequestDataException` is returned if expression starts with a special character
/// Clause: 5.7.1, 4.21
#[gtest]
#[tokio::test]
async fn e018_18_08_retrieve_with_expression_starting_with_special_character() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = created_building(&broker, &namespace, &LOCATED_AT_TERMS).await?;
    let id = namespace.urn("Building", 1)?;
    refused_query(&broker.with_context(vocabulary.context())?, &id, "pick=id%2ClocatedAt%7B%2Cname%7D").await?;
    delete_entities(&broker, &[&id]).await;
    Ok(())
}

/// Pins: Check that a `BadRequestDataException` is returned if expression does not contain an attribute
/// Clause: 5.7.1, 4.21
#[gtest]
#[tokio::test]
async fn e018_18_09_retrieve_with_expression_containing_no_attribute() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = created_building(&broker, &namespace, &LOCATED_AT_TERMS).await?;
    let id = namespace.urn("Building", 1)?;
    refused_query(&broker.with_context(vocabulary.context())?, &id, "pick=id%2ClocatedAt%7B%7D").await?;
    delete_entities(&broker, &[&id]).await;
    Ok(())
}
