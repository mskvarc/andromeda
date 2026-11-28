//! A reloaded term defined as a boolean is ld context not available.

use andromeda_test_suite::{broker, cached_revision::CachedRevision, namespace::Namespace, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{http::operation::ContextDeletion, model::ErrorType};

/// Pins: Delete @context with reload=true of a Cached @context whose origin now serves a document defining the term
/// speed as a boolean answers 504 `LdContextNotAvailable` with a detail naming the URL and what is wrong and where, and
/// the entry keeps the first download: still Cached, created when it was, and a request naming the URL reads `speed` by
/// it
/// Clause: 5.13.5.4, 5.5.4, JSON-LD 1.1 section 9.15
#[gtest]
#[tokio::test]
async fn n_ctx_del_9009_a_reloaded_term_defined_as_a_boolean_is_ld_context_not_available() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let cached = CachedRevision::recorded(&broker, &namespace, r#"{"@context": {"speed": true}}"#).await?;
    let answer = broker
        .with_responses()
        .contexts()
        .delete(cached.local_id())
        .deletion(ContextDeletion::Reload)
        .send()
        .await;
    expect_that!(
        &answer,
        problem(StatusCode::GATEWAY_TIMEOUT, ErrorType::LdContextNotAvailable).with_detail(&format!(
            r#"the context downloaded from {} is not valid JSON-LD: the @context defines the term "speed" as a boolean, but a term definition is null, a string or an object"#,
            cached.url()
        ))
    );
    cached.verify_kept(&broker).await
}
