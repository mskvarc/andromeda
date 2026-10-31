//! An entity is not retrieved with a `format` or `options` value the specification does not define, nor with one
//! invalid query parameter beside valid ones (clauses 5.7.1, 6.3.7 and 6.3.20).

use andromeda_test_suite::{broker, namespace::Namespace, problem::problem, wire::request_to};
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::ACCEPT};
use ngsi_ld::model::ErrorType;

/// Pins: Check that retrieving an entity with invalid format query param returns an error
/// Clause: 5.7.1, 6.3.7, 6.3.20
/// Vocabulary: none (the test references no @context)
#[gtest]
#[tokio::test]
async fn e018_16_01_retrive_with_invalid_format() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let id = namespace.urn("Building", 1)?;
    let request = request_to(&broker, Method::GET, &format!("entities/{id}?format=invalidFormat"))?
        .append_header(ACCEPT, HeaderValue::from_static("application/ld+json"));
    expect_that!(
        &broker.send_unchecked(request).await,
        problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest)
    );
    Ok(())
}

/// Pins: Check that retrieving an entity with invalid options query param returns an error
/// Clause: 5.7.1, 6.3.7, 6.3.20
/// Vocabulary: none (the test references no @context)
#[gtest]
#[tokio::test]
async fn e018_16_02_retrive_with_invalid_options() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let id = namespace.urn("Building", 1)?;
    let request = request_to(&broker, Method::GET, &format!("entities/{id}?options=invalidOptions"))?
        .append_header(ACCEPT, HeaderValue::from_static("application/ld+json"));
    expect_that!(
        &broker.send_unchecked(request).await,
        problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest)
    );
    Ok(())
}

/// Pins: Check that retrieving an entity with at least one invalid options query param returns an error
/// Clause: 5.7.1, 6.3.7, 6.3.20
/// Vocabulary: none (the test references no @context)
#[gtest]
#[tokio::test]
async fn e018_16_03_retrive_with_at_least_one_invalid_options() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let id = namespace.urn("Building", 1)?;
    let request = request_to(&broker, Method::GET, &format!("entities/{id}?options=keyValues%2Cinvalid"))?
        .append_header(ACCEPT, HeaderValue::from_static("application/ld+json"));
    expect_that!(
        &broker.send_unchecked(request).await,
        problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest)
    );
    Ok(())
}

/// Pins: Check that retrieving an entity with one invalid query param returns an error
/// Clause: 5.7.1, 6.3.7, 6.3.20
/// Vocabulary: none (the test references no @context)
#[gtest]
#[tokio::test]
async fn e018_16_04_retrive_with_one_invalid_query_param() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let id = namespace.urn("Building", 1)?;
    let request = request_to(&broker, Method::GET, &format!("entities/{id}?options=invalid&format=keyValues"))?
        .append_header(ACCEPT, HeaderValue::from_static("application/ld+json"));
    expect_that!(
        &broker.send_unchecked(request).await,
        problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest)
    );
    Ok(())
}
