//! A reload of a cached `@context` whose server is gone fails with 504, and the cached copy stays, served with its
//! details (clauses 5.13.4 and 5.13.5).

use crate::jsonld_context::{
    stored_contexts::{attribute_terms_document, eventual_details, expect_context_details, served_details},
    testing_entity::{post_in_context, testing_entity},
};
use andromeda_test_suite::{broker, namespace::Namespace, problem::answered, remote_context::RemoteContexts, teardown::AnyAnswer, wire};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::operation::ContextDeletion,
    model::{ContextId, StoredContextKind},
};

/// Pins: Check that the context served by a context server is still in the broker after a
/// `ERROR_TYPE_LD_CONTEXT_NOT_AVAILABLE` with details=true
/// Clause: 5.13.4
/// Isolation: the @context server is the test's own, serving the document at `contexts/<ns>/api/v1/context.jsonld` with
/// no Content-Type
/// Timing: the Serve that reads the download is retried for up to 3 s (12 times 250 ms) and ends once the broker has
/// recorded the download
/// Expected: lastUsage and numberOfHits are absent or of their type (a `DateTime` and a number, clause 5.13.3.5)
/// Vocabulary: none (the test is about @context storage itself and needs no vocabulary)
#[gtest]
#[tokio::test]
async fn e053_05_01_cached_context_stays_after_ld_context_not_available() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let remote = RemoteContexts::start().await?;
    let server = remote
        .serve("api/v1/context.jsonld", &attribute_terms_document(&namespace, &["A1", "A2", "A3"]), None)
        .await?;
    let testing = testing_entity(&namespace, "randomUUID")?;
    post_in_context(&broker, &testing, &server).await?;
    // The client writes Boolean parameters in lower case only, so the listing with `details=True` is sent as written;
    // its answer is not checked.
    broker
        .send_unchecked(wire::request_to(&broker, Method::GET, "jsonldContexts?details=True")?)
        .await
        .any_answer();
    let id: ContextId = server.as_str().parse()?;
    let downloaded = eventual_details(&broker, &id, |details| details.kind == StoredContextKind::Cached).await?;
    expect_that!(downloaded.kind, eq(&StoredContextKind::Cached));
    remote.stop();
    let reloaded = broker.with_responses().contexts().delete(&id).deletion(ContextDeletion::Reload).send().await;
    expect_that!(&reloaded, answered(StatusCode::GATEWAY_TIMEOUT));
    // The client writes Boolean parameters in lower case only, so the listing with `details=True` is sent as written;
    // its answer is not checked.
    broker
        .send_unchecked(wire::request_to(&broker, Method::GET, "jsonldContexts?details=True")?)
        .await
        .any_answer();
    let details = served_details(&broker, &id).await?;
    expect_context_details(&details, &StoredContextKind::Cached);
    broker.entities().delete(&testing.id).send().await.any_answer();
    broker.temporal().delete(&testing.id).send().await.any_answer();
    broker.contexts().delete(&id).send().await.any_answer();
    Ok(())
}
