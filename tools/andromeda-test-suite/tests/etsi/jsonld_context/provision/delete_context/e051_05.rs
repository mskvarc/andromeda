//! A reload of a cached `@context` whose server is gone is refused with 504 `LdContextNotAvailable` (clause 5.13.5).

use crate::jsonld_context::{
    stored_contexts::{attribute_terms_document, eventual_details},
    testing_entity::{post_in_context, testing_entity},
};
use andromeda_test_suite::{broker, namespace::Namespace, problem::problem, remote_context::RemoteContexts, teardown::AnyAnswer, wire};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::operation::ContextDeletion,
    model::{ContextId, ErrorType, StoredContextKind},
};

/// Pins: Check that one gets an error if one tries to reload a cached context with no communication with the context
/// server
/// Clause: 5.13.5
/// Isolation: the @context server is the test's own, serving the document at `contexts/<ns>/api/v1/context.jsonld` with
/// no Content-Type
/// Timing: the Serve that reads the download is retried for up to 3 s (12 times 250 ms) and ends once the broker has
/// recorded the download
/// Vocabulary: none (the test is about @context storage itself and needs no vocabulary)
#[gtest]
#[tokio::test]
async fn e051_05_01_delete_and_reload_a_cached_context_without_the_context_server() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let remote = RemoteContexts::start().await?;
    let server = remote
        .serve("api/v1/context.jsonld", &attribute_terms_document(&namespace, &["A1", "A2", "A3"]), None)
        .await?;
    let testing = testing_entity(&namespace, "randomUUID")?;
    post_in_context(&broker, &testing, &server).await?;
    broker
        .send_unchecked(wire::request_to(&broker, Method::GET, "jsonldContexts")?)
        .await
        .any_answer();
    let id: ContextId = server.as_str().parse()?;
    let downloaded = eventual_details(&broker, &id, |details| details.kind == StoredContextKind::Cached).await?;
    expect_that!(downloaded.kind, eq(&StoredContextKind::Cached));
    remote.stop();
    let reloaded = broker.with_responses().contexts().delete(&id).deletion(ContextDeletion::Reload).send().await;
    expect_that!(
        &reloaded,
        problem(StatusCode::GATEWAY_TIMEOUT, ErrorType::LdContextNotAvailable).with_distinct_detail()
    );
    broker.entities().delete(&testing.id).send().await.any_answer();
    broker.temporal().delete(&testing.id).send().await.any_answer();
    broker.contexts().delete(&id).send().await.any_answer();
    Ok(())
}
