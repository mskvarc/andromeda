//! The `numberOfHits` of a cached `@context` counts the requests that use it (clauses 5.13.3.5 and 5.13.4).

use crate::jsonld_context::{
    stored_contexts::{attribute_terms_document, eventual_details, served_details},
    testing_entity::{post_in_context, testing_entity},
};
use andromeda_test_suite::{broker, namespace::Namespace, remote_context::RemoteContexts, teardown::AnyAnswer};
use googletest::prelude::*;
use ngsi_ld::model::ContextId;

/// Pins: Check that the numberOfHits is increased after using a Cached context
/// Clause: 5.13.4
/// Expected: numberOfHits counts uses of the @context, not serves of its metadata (clause 5.13.3.5)
/// Isolation: the @context server is the test's own, serving the document at `contexts/<ns>/api/v1/context.jsonld` with
/// no Content-Type
/// Timing: the request that reads a Cached @context right after its download is retried for up to 3 s (12 times 250
/// ms): Andromeda records a downloaded @context after the request that downloaded it
/// Expected: numberOfHits is absent or the expected number (clause 5.13.3.5 makes it a number)
/// Vocabulary: none (the test is about @context storage itself and needs no vocabulary)
#[gtest]
#[tokio::test]
async fn e053_06_01_number_of_hits_increases_after_using_a_cached_context() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let remote = RemoteContexts::start().await?;
    let server = remote
        .serve("api/v1/context.jsonld", &attribute_terms_document(&namespace, &["A1", "A2", "A3"]), None)
        .await?;
    let first = testing_entity(&namespace, 1)?;
    post_in_context(&broker, &first, &server).await?;
    let id: ContextId = server.as_str().parse()?;
    let once = eventual_details(&broker, &id, |details| details.number_of_hits.is_none_or(|hits| hits == 1)).await?;
    expect_that!(once.number_of_hits, any![none(), some(eq(1))]);
    post_in_context(&broker, &testing_entity(&namespace, 2)?, &server).await?;
    let twice = served_details(&broker, &id).await?;
    expect_that!(twice.number_of_hits, any![none(), some(eq(2))]);
    broker.entities().delete(&first.id).send().await.any_answer();
    broker.temporal().delete(&first.id).send().await.any_answer();
    broker.contexts().delete(&id).send().await.any_answer();
    remote.stop();
    Ok(())
}
