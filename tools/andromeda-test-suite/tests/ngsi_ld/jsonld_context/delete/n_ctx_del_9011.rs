//! A reloaded redefined type keyword is ld context not available.

use andromeda_test_suite::{broker, cached_revision::CachedRevision, namespace::Namespace, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{http::operation::ContextDeletion, model::ErrorType};

/// Pins: Delete @context with reload=true of a Cached @context whose origin now serves a document redefining the
/// keyword @type as a string answers 504 `LdContextNotAvailable` with a detail naming the URL and what is wrong and
/// where, and the entry keeps the first download: still Cached, created when it was, and a request naming the URL reads
/// `speed` by it
/// Clause: 5.13.5.4, 5.5.4, JSON-LD 1.1 section 9.15
#[gtest]
#[tokio::test]
async fn n_ctx_del_9011_a_reloaded_redefined_type_keyword_is_ld_context_not_available() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let cached = CachedRevision::recorded(&broker, &namespace, r#"{"@context": {"@type": "https://example.org/t"}}"#).await?;
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
            "the context downloaded from {} is not valid JSON-LD: the @context has a member @type that is a string, but an object is expected",
            cached.url()
        ))
    );
    cached.verify_kept(&broker).await
}
