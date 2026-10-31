//! A temporal evolution query whose `format` or `options` value the specification does not define, or which carries one
//! such value beside valid ones, is refused with 400 `InvalidRequest` (clauses 5.7.3, 6.3.12 and 6.3.20).

use andromeda_test_suite::{broker, namespace::Namespace, problem::problem, vocabulary::Vocabulary, wire::request_to};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::model::ErrorType;

/// Pins: Check that one cannot query with an invalid format query param
/// Clause: 5.7.3, 6.3.12, 6.3.20
#[gtest]
#[tokio::test]
async fn e021_20_01_query_with_invalid_format() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Vehicle"]).await?;

    let client = broker.with_context(vocabulary.context())?;
    let request = request_to(
        &client,
        Method::GET,
        "temporal/entities?type=Vehicle&timerel=after&timeAt=2020-01-01T12%3A03%3A00Z&format=invalidFormat",
    )?;
    expect_that!(
        &client.send_unchecked(request).await,
        problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest)
    );
    Ok(())
}

/// Pins: Check that one cannot query with an invalid options query param
/// Clause: 5.7.3, 6.3.12, 6.3.20
#[gtest]
#[tokio::test]
async fn e021_20_02_query_with_invalid_options() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Vehicle"]).await?;

    let client = broker.with_context(vocabulary.context())?;
    let request = request_to(
        &client,
        Method::GET,
        "temporal/entities?type=Vehicle&timerel=after&timeAt=2020-01-01T12%3A03%3A00Z&options=invalidOptions",
    )?;
    expect_that!(
        &client.send_unchecked(request).await,
        problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest)
    );
    Ok(())
}

/// Pins: Check that one cannot query with at least one invalid options query param
/// Clause: 5.7.3, 6.3.12, 6.3.20
#[gtest]
#[tokio::test]
async fn e021_20_03_query_with_at_least_one_invalid_options() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Vehicle"]).await?;

    let client = broker.with_context(vocabulary.context())?;
    let request = request_to(
        &client,
        Method::GET,
        "temporal/entities?type=Vehicle&timerel=after&timeAt=2020-01-01T12%3A03%3A00Z&options=temporalValues%2CinvalidOptions",
    )?;
    expect_that!(
        &client.send_unchecked(request).await,
        problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest)
    );
    Ok(())
}

/// Pins: Check that one cannot query with at least one invalid query param
/// Clause: 5.7.3, 6.3.12, 6.3.20
#[gtest]
#[tokio::test]
async fn e021_20_04_query_with_one_invalid_query_param() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Vehicle"]).await?;

    let client = broker.with_context(vocabulary.context())?;
    let request = request_to(
        &client,
        Method::GET,
        "temporal/entities?type=Vehicle&timerel=after&timeAt=2020-01-01T12%3A03%3A00Z&options=aggregatedValues%2CinvalidOptions&format=temporalValues",
    )?;
    expect_that!(
        &client.send_unchecked(request).await,
        problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest)
    );
    Ok(())
}
