//! Serving the details of a `@context` the broker does not store is refused with 404 `ResourceNotFound` (clause
//! 5.13.4).

use crate::jsonld_context::stored_contexts::{add_context, attribute_terms};
use andromeda_test_suite::{broker, namespace::Namespace, problem::problem, teardown::AnyAnswer, wire};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::model::ErrorType;

/// Pins: Check that an error is returned when one requests for a @context that does not exist
/// Clause: 5.13.4
/// Vocabulary: none (the test is about @context storage itself and needs no vocabulary)
#[gtest]
#[tokio::test]
async fn e053_03_01_serve_a_contexts_with_a_wrong_id_and_correct_details() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let added = add_context(&broker, &attribute_terms(&namespace, &["A1", "A2", "A3"])?).await?;
    // Hand-written: `wrong_id_context` is no URI, which a typed identifier refuses.
    let request = wire::request_to(&broker, Method::GET, "jsonldContexts/wrong_id_context?details=true")?;
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound).with_distinct_detail());
    broker.contexts().delete(&added.id).send().await.any_answer();
    Ok(())
}
