//! ETSI `051_05`: a reload of a cached `@context` whose server is gone is refused with 504 `LdContextNotAvailable`
//! (clause 5.13.5).

use crate::jsonld_context::{
    stored_contexts::{attribute_terms_document, eventual_details},
    testing_entity::{post_in_context, testing_entity},
};
use andromeda_test_suite::{broker, namespace::Namespace, problem::problem, remote_context::RemoteContexts, teardown::AnyAnswer, wire};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::operation::ContextDeletion,
    model::{ContextId, ErrorType},
};
use serde_json::json;

/// Pins: Check that one gets an error if one tries to reload a cached context with no communication with the context
/// server
/// Clause: 5.13.5
/// Changed: the suite's @context server (Start @context Local Server at
/// `http://${context_server_host}:${context_server_port}/api/v1/context.jsonld`) is a WireMock stub at
/// `{{mock_internal}}/contexts/{{ns}}/api/v1/context.jsonld` answering the same file text with no Content-Type, as the
/// `HttpCtrl` stub reply does
/// Changed: Sleep 3s before reading the download becomes a retry of the Serve that reads it, for up to 3 s (12 times
/// 250 ms): the lint forbids delay, and the retry ends once the broker has recorded the download, within the same bound
/// Changed: Stop @context Local Server makes the stub reset every connection (a WireMock fault), so the @context can no
/// longer be downloaded, as when the `HttpCtrl` server stops listening
/// Changed: Check Response Reason set to compares the HTTP/1.1 reason phrase, which Hurl cannot read; RFC 9112
/// clause 4 has a client ignore it and HTTP/2 carries none (RFC 9113 clause 8.3.2), so the port checks the status code
/// the phrase names
/// Vocabulary: none (the case is about @context storage itself and references no suite @context)
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
    let downloaded = eventual_details(&broker, &id, |details| details.get("kind") == Some(&json!("Cached"))).await?;
    expect_that!(downloaded.get("kind"), some(eq(&json!("Cached"))));
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
