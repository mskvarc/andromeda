//! ETSI `019_18`: a query is refused with `BadRequestData` when `pick` and `omit` name the same member, when `attrs` is
//! sent beside `pick` or `omit`, or when the projection is malformed (clauses 5.7.1, 5.7.2 and 4.21).

use crate::{
    suite_keywords::{create_eiffel_vehicle_pisa, delete_eiffel_vehicle_pisa, refused_get},
    suite_payloads::BUILDING_VEHICLE_TERMS,
};
use andromeda_test_suite::{
    broker::{self},
    namespace::Namespace,
    problem::problem,
    vocabulary::Vocabulary,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::ErrorType,
    query::{SelectorType, projection},
};

/// The terms of the `@context` of the cases that name `category` in the query.
const CATEGORY_TERMS: [&str; 11] = [
    "Building",
    "name",
    "subCategory",
    "airQualityLevel",
    "almostFull",
    "Vehicle",
    "brandName",
    "isParked",
    "providedBy",
    "locatedIn",
    "category",
];

/// The terms of the `@context` of the cases that name `locatedAt` in the query.
const LOCATED_AT_TERMS: [&str; 11] = [
    "Building",
    "name",
    "subCategory",
    "airQualityLevel",
    "almostFull",
    "Vehicle",
    "brandName",
    "isParked",
    "providedBy",
    "locatedIn",
    "locatedAt",
];

/// Pins: Check that a `BadRequestDataException` is returned if an entity member is present in pick and omit
/// Clause: 5.7.2, 4.21
/// Defect: D-80: Andromeda admits a member named in both pick and omit instead of refusing it with `BadRequestData`;
/// GET entities?pick=name&omit=name answers 200 []
#[gtest]
#[tokio::test]
async fn e019_18_01_retrieve_with_same_entity_member_in_pick_and_omit() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &BUILDING_VEHICLE_TERMS).await?;
    create_eiffel_vehicle_pisa(&broker, &vocabulary, &namespace).await?;
    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
        .with_responses()
        .entities()
        .query_projected()
        .entity_type("Building,Vehicle".parse::<SelectorType>()?)
        .pick(projection::attributes(["name"]).build()?)
        .omit(projection::attributes(["name"]).build()?)
        .send()
        .await;
    expect_that!(&received, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    delete_eiffel_vehicle_pisa(&broker, &namespace).await?;
    Ok(())
}

/// Pins: Check that a `BadRequestDataException` is returned if pick and attrs query params are present
/// Clause: 5.7.2, 4.21
#[gtest]
#[tokio::test]
async fn e019_18_02_retrieve_with_pick_and_attrs() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &CATEGORY_TERMS).await?;
    create_eiffel_vehicle_pisa(&broker, &vocabulary, &namespace).await?;
    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
        .with_responses()
        .entities()
        .query_projected()
        .entity_type("Building,Vehicle".parse::<SelectorType>()?)
        .attrs(vec!["category".parse()?])
        .pick(projection::attributes(["name"]).build()?)
        .send()
        .await;
    expect_that!(&received, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    delete_eiffel_vehicle_pisa(&broker, &namespace).await?;
    Ok(())
}

/// Pins: Check that a `BadRequestDataException` is returned if omit and attrs query params are present
/// Clause: 5.7.2, 4.21
#[gtest]
#[tokio::test]
async fn e019_18_03_retrieve_with_omit_and_attrs() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &CATEGORY_TERMS).await?;
    create_eiffel_vehicle_pisa(&broker, &vocabulary, &namespace).await?;
    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
        .with_responses()
        .entities()
        .query()
        .entity_type("Building,Vehicle".parse::<SelectorType>()?)
        .attrs(vec!["category".parse()?])
        .omit(projection::attributes(["name"]).build()?)
        .send()
        .await;
    expect_that!(&received, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    delete_eiffel_vehicle_pisa(&broker, &namespace).await?;
    Ok(())
}

/// Pins: Check that a `BadRequestDataException` is returned if an invalid character is present
/// Clause: 5.7.1, 4.21
#[gtest]
#[tokio::test]
async fn e019_18_04_retrieve_with_invalid_character() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &BUILDING_VEHICLE_TERMS).await?;
    create_eiffel_vehicle_pisa(&broker, &vocabulary, &namespace).await?;
    refused_get(&broker.with_context(vocabulary.context())?, "entities/?type=Building%2CVehicle&pick=id%3Bname").await?;
    delete_eiffel_vehicle_pisa(&broker, &namespace).await?;
    Ok(())
}

/// Pins: Check that a `BadRequestDataException` is returned if a brace is not closed
/// Clause: 5.7.1, 4.21
#[gtest]
#[tokio::test]
async fn e019_18_05_retrieve_with_unclosed_brace() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &LOCATED_AT_TERMS).await?;
    create_eiffel_vehicle_pisa(&broker, &vocabulary, &namespace).await?;
    refused_get(
        &broker.with_context(vocabulary.context())?,
        "entities/?type=Building%2CVehicle&pick=id%2ClocatedAt%7Bname",
    )
    .await?;
    delete_eiffel_vehicle_pisa(&broker, &namespace).await?;
    Ok(())
}

/// Pins: Check that a `BadRequestDataException` is returned if a double brace is present
/// Clause: 5.7.1, 4.21
#[gtest]
#[tokio::test]
async fn e019_18_06_retrieve_with_double_braces() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &LOCATED_AT_TERMS).await?;
    create_eiffel_vehicle_pisa(&broker, &vocabulary, &namespace).await?;
    refused_get(
        &broker.with_context(vocabulary.context())?,
        "entities/?type=Building%2CVehicle&pick=id%2ClocatedAt%7B%7Bname%7D",
    )
    .await?;
    delete_eiffel_vehicle_pisa(&broker, &namespace).await?;
    Ok(())
}

/// Pins: Check that a `BadRequestDataException` is returned if consecutive separators are present
/// Clause: 5.7.1, 4.21
#[gtest]
#[tokio::test]
async fn e019_18_07_retrieve_with_consecutive_separators() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &BUILDING_VEHICLE_TERMS).await?;
    create_eiffel_vehicle_pisa(&broker, &vocabulary, &namespace).await?;
    refused_get(
        &broker.with_context(vocabulary.context())?,
        "entities/?type=Building%2CVehicle&pick=id%2C%2Cname",
    )
    .await?;
    delete_eiffel_vehicle_pisa(&broker, &namespace).await?;
    Ok(())
}

/// Pins: Check that a `BadRequestDataException` is returned if expression starts with a special character
/// Clause: 5.7.1, 4.21
#[gtest]
#[tokio::test]
async fn e019_18_08_retrieve_with_expression_starting_with_special_character() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &LOCATED_AT_TERMS).await?;
    create_eiffel_vehicle_pisa(&broker, &vocabulary, &namespace).await?;
    refused_get(
        &broker.with_context(vocabulary.context())?,
        "entities/?type=Building%2CVehicle&pick=id%2ClocatedAt%7B%2Cname%7D",
    )
    .await?;
    delete_eiffel_vehicle_pisa(&broker, &namespace).await?;
    Ok(())
}

/// Pins: Check that a `BadRequestDataException` is returned if expression does not contain an attribute
/// Clause: 5.7.1, 4.21
#[gtest]
#[tokio::test]
async fn e019_18_09_retrieve_with_expression_containing_no_attribute() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &LOCATED_AT_TERMS).await?;
    create_eiffel_vehicle_pisa(&broker, &vocabulary, &namespace).await?;
    refused_get(
        &broker.with_context(vocabulary.context())?,
        "entities/?type=Building%2CVehicle&pick=id%2ClocatedAt%7B%7D",
    )
    .await?;
    delete_eiffel_vehicle_pisa(&broker, &namespace).await?;
    Ok(())
}
