//! ETSI `051_04`: deleting a `@context` the broker does not store is refused with 404 `ResourceNotFound` whatever
//! valid `reload` is given, a `reload` that is no boolean with 400 `BadRequestData`, and a reload of a hosted
//! `@context` with 400 `BadRequestData` (clause 5.13.5).

use crate::jsonld_context::stored_contexts::{StoredContext, add_context, attribute_terms};
use andromeda_test_suite::{
    broker::{self, BrokerClient},
    namespace::Namespace,
    problem::problem,
    teardown::AnyAnswer,
    wire,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{http::operation::ContextDeletion, model::ErrorType};

/// `Create Initial @context`: adds the `A1`..`A3` context, required to answer 201 with a `Location`.
async fn initial_context(broker: &BrokerClient, namespace: &Namespace) -> Result<StoredContext> {
    add_context(broker, &attribute_terms(namespace, &["A1", "A2", "A3"])?).await
}

/// Adds the initial context, sends `DELETE jsonldContexts/wrong_id_context<query>`, hand-written because
/// `wrong_id_context` is no URI, which a typed identifier refuses, requires `status` with a problem of `error_type`,
/// a non-empty title and a non-empty detail that differs from it, and deletes the initial context.
async fn refused_wrong_id(query: &str, status: StatusCode, error_type: ErrorType) -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let added = initial_context(&broker, &namespace).await?;
    let request = wire::request_to(&broker, Method::DELETE, &format!("jsonldContexts/wrong_id_context{query}"))?;
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(status, error_type).with_distinct_detail());
    broker.contexts().delete(&added.id).send().await.any_answer();
    Ok(())
}

/// Pins: Check that one can delete a hosted @context
/// Clause: 5.13.5
/// Changed: Check Response Reason set to compares the HTTP/1.1 reason phrase, which Hurl cannot read; RFC 9112
/// clause 4 has a client ignore it and HTTP/2 carries none (RFC 9113 clause 8.3.2), so the port checks the status code
/// the phrase names
/// Vocabulary: none (the case is about @context storage itself and references no suite @context)
#[gtest]
#[tokio::test]
async fn e051_04_01_delete_a_contexts_with_a_wrong_id_and_reload_set_to_true() -> Result<()> {
    refused_wrong_id("?reload=true", StatusCode::NOT_FOUND, ErrorType::ResourceNotFound).await
}

/// Pins: Check that one can delete a hosted @context
/// Clause: 5.13.5
/// Changed: Check Response Reason set to compares the HTTP/1.1 reason phrase, which Hurl cannot read; RFC 9112
/// clause 4 has a client ignore it and HTTP/2 carries none (RFC 9113 clause 8.3.2), so the port checks the status code
/// the phrase names
/// Vocabulary: none (the case is about @context storage itself and references no suite @context)
#[gtest]
#[tokio::test]
async fn e051_04_02_delete_a_contexts_with_a_wrong_id_and_reload_set_to_false() -> Result<()> {
    refused_wrong_id("?reload=false", StatusCode::NOT_FOUND, ErrorType::ResourceNotFound).await
}

/// Pins: Check that one can delete a hosted @context
/// Clause: 5.13.5
/// Changed: Check Response Reason set to compares the HTTP/1.1 reason phrase, which Hurl cannot read; RFC 9112
/// clause 4 has a client ignore it and HTTP/2 carries none (RFC 9113 clause 8.3.2), so the port checks the status code
/// the phrase names
/// Vocabulary: none (the case is about @context storage itself and references no suite @context)
#[gtest]
#[tokio::test]
async fn e051_04_03_delete_a_contexts_with_a_wrong_id_and_no_reload_value() -> Result<()> {
    refused_wrong_id("", StatusCode::NOT_FOUND, ErrorType::ResourceNotFound).await
}

/// Pins: Check that one can delete a hosted @context
/// Clause: 5.13.5
/// Changed: Check Response Reason set to compares the HTTP/1.1 reason phrase, which Hurl cannot read; RFC 9112
/// clause 4 has a client ignore it and HTTP/2 carries none (RFC 9113 clause 8.3.2), so the port checks the status code
/// the phrase names
/// Vocabulary: none (the case is about @context storage itself and references no suite @context)
#[gtest]
#[tokio::test]
async fn e051_04_04_delete_a_contexts_with_a_wrong_id_and_wrong_reload_value() -> Result<()> {
    refused_wrong_id("?reload=xxx", StatusCode::BAD_REQUEST, ErrorType::BadRequestData).await
}

/// Pins: Check that one can delete a hosted @context
/// Clause: 5.13.5
/// Changed: Check Response Reason set to compares the HTTP/1.1 reason phrase, which Hurl cannot read; RFC 9112
/// clause 4 has a client ignore it and HTTP/2 carries none (RFC 9113 clause 8.3.2), so the port checks the status code
/// the phrase names
/// Vocabulary: none (the case is about @context storage itself and references no suite @context)
#[gtest]
#[tokio::test]
async fn e051_04_05_delete_a_hosted_contexts_with_a_valid_id_and_reload_set_to_true() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let added = initial_context(&broker, &namespace).await?;
    let reloaded = broker
        .with_responses()
        .contexts()
        .delete(&added.id)
        .deletion(ContextDeletion::Reload)
        .send()
        .await;
    expect_that!(&reloaded, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_distinct_detail());
    broker.contexts().delete(&added.id).send().await.any_answer();
    Ok(())
}
