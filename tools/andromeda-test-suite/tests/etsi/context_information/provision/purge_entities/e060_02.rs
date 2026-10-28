//! ETSI `060_02`: a Purge Entities that names both `keep` and `drop` is refused with 400 `BadRequestData` (clauses 5.6.21
//! and 6.4.3.3).

use andromeda_test_suite::{broker, namespace::Namespace, problem::problem, vocabulary::Vocabulary};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::model::{AttributeName, ErrorType};

/// The terms of the `@context` the case registers.
const TERMS: [&str; 3] = ["Building", "name", "subCategory"];

/// Pins: Check that a `BadRequestData` error is returned when both keep and drop query parameters are provided
/// Clause: 5.6.21, 6.4.3.3
#[gtest]
#[tokio::test]
async fn e060_02_01_purge_entities_with_keep_and_drop_at_the_same_time() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &TERMS).await?;
    let client = broker.with_context(vocabulary.context())?;
    let answer = client
        .with_responses()
        .entities()
        .purge()
        .entity_type(namespace.expanded_term("Building")?.as_str())
        .keep(vec!["name".parse::<AttributeName>()?])
        .drop(vec!["subCategory".parse::<AttributeName>()?])
        .send()
        .await;
    verify_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title())
}
