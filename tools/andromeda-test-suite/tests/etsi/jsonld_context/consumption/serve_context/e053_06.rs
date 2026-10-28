//! ETSI `053_06`: the `numberOfHits` of a cached `@context` counts the requests that use it (clauses 5.13.3.5 and
//! 5.13.4).

use crate::jsonld_context::{
    stored_contexts::{attribute_terms_document, eventual_details, served_details},
    testing_entity::{post_in_context, testing_entity},
};
use andromeda_test_suite::{broker, namespace::Namespace, remote_context::RemoteContexts, teardown::AnyAnswer};
use googletest::prelude::*;
use ngsi_ld::model::ContextId;
use serde_json::{Value, json};

/// Pins: Check that the numberOfHits is increased after using a Cached context
/// Clause: 5.13.4
/// Corrected: fix(numberofhits): count context uses, not metadata serves (5.13.3.5)
/// Changed: the suite's @context server (Start @context Local Server at
/// `http://${context_server_host}:${context_server_port}/api/v1/context.jsonld`) is a WireMock stub at
/// `{{mock_internal}}/contexts/{{ns}}/api/v1/context.jsonld` answering the same file text with no Content-Type, as the
/// `HttpCtrl` stub reply does
/// Changed: the request that reads a Cached @context right after its download is retried for up to 3 s (12 times
/// 250 ms), the wait the suite itself gives a download to be recorded in `051_05` and `053_05`: Andromeda records a
/// downloaded @context after the request that downloaded it
/// Changed: numberOfHits is required to be absent or the expected number, where the keyword also skips a null or empty
/// value and compares a text as a number; clause 5.13.3.5 makes it a number
/// Vocabulary: none (the case is about @context storage itself and references no suite @context)
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
    let once = eventual_details(&broker, &id, |details| details.get("numberOfHits").is_none_or(|hits| *hits == json!(1))).await?;
    expect_that!(once.get("numberOfHits"), any![none(), some(eq(&json!(1)))]);
    post_in_context(&broker, &testing_entity(&namespace, 2)?, &server).await?;
    let twice = serde_json::from_slice::<Value>(served_details(&broker, &id).await?.body())?;
    expect_that!(twice.get("numberOfHits"), any![none(), some(eq(&json!(2)))]);
    broker.entities().delete(&first.id).send().await.any_answer();
    broker.temporal().delete(&first.id).send().await.any_answer();
    broker.contexts().delete(&id).send().await.any_answer();
    remote.stop();
    Ok(())
}
