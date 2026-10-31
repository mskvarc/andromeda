//! A hosted `@context` is served, with no `details` or `details=false`, as the document that was added, in
//! `application/json` (clause 5.13.4).

use crate::jsonld_context::stored_contexts::{add_context, attribute_terms};
use andromeda_test_suite::{broker, comparison::matches_document, namespace::Namespace, problem::answered, teardown::AnyAnswer, wire};
use googletest::prelude::*;
use http::{StatusCode, header::CONTENT_TYPE};
use ngsi_ld::http::{headers::Accept, operation::SummaryDetails};
use serde_json::json;

/// Adds the `A1`..`A3` context, serves it with `details`, and requires 200 in `application/json` with exactly the
/// added document.
async fn serves_the_added_context(details: SummaryDetails) -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let added = add_context(&broker, &attribute_terms(&namespace, &["A1", "A2", "A3"])?).await?;
    let served = broker
        .with_accept(Accept::any())
        .with_responses()
        .contexts()
        .serve(&added.id)
        .details(details)
        .send()
        .await;
    expect_that!(&served, answered(StatusCode::OK));
    let served = served?;
    expect_that!(wire::header_value(served.head.headers(), &CONTENT_TYPE), some(eq("application/json")));
    let token = namespace.token();
    expect_that!(
        wire::json_body(&served)?,
        matches_document(json!({
            "@context": {
                "A1": format!("urn:ngsi-ld:attributes:{token}-A1"),
                "A2": format!("urn:ngsi-ld:attributes:{token}-A2"),
                "A3": format!("urn:ngsi-ld:attributes:{token}-A3"),
            }
        }))
    );
    broker.contexts().delete(&added.id).send().await.any_answer();
    Ok(())
}

/// Pins: Check that one can serve a @context with details equal to empty or false
/// Clause: 5.13.4
/// Vocabulary: none (the test is about @context storage itself and needs no vocabulary)
#[gtest]
#[tokio::test]
async fn e053_01_01_serve_a_context_without_details() -> Result<()> {
    serves_the_added_context(SummaryDetails::Implied).await
}

/// Pins: Check that one can serve a @context with details equal to empty or false
/// Clause: 5.13.4
/// Vocabulary: none (the test is about @context storage itself and needs no vocabulary)
#[gtest]
#[tokio::test]
async fn e053_01_02_serve_a_context_with_details_equal_to_false() -> Result<()> {
    serves_the_added_context(SummaryDetails::Explicit).await
}
