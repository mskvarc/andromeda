//! Reading what a broker forwarded to the test's context source: the wait for a forwarded request, its body and its
//! query parameters, and the `@context` reading of an entity the broker answers with.
//!
//! A wait whose request a stub has already answered keeps its 200 reply for the next request no stub matches: the wait
//! queues that one reply on the source.

use andromeda_test_suite::context_source::ContextSource;
use googletest::prelude::*;
use http::StatusCode;
use serde_json::Value;
use wiremock::{Mock, Request, ResponseTemplate, matchers::any};

/// The priority of the queued reply: below every stub, above the source's answer to an unmatched request.
const QUEUED_REPLY_PRIORITY: u8 = u8::MAX - 1;

/// Waits until the source holds a request beyond the `taken` earlier waits took, queues the 200 reply for the next
/// request no stub matches, and answers the last request the source holds.
pub(crate) async fn wait_for_redirected(source: &ContextSource, taken: usize) -> Result<Request> {
    let request = wait_for_request(source, taken).await?;
    source
        .mount(
            Mock::given(any())
                .respond_with(ResponseTemplate::new(StatusCode::OK))
                .up_to_n_times(1)
                .with_priority(QUEUED_REPLY_PRIORITY),
        )
        .await?;
    Ok(request)
}

/// Waits until the source holds a request beyond the `taken` earlier waits took, and answers the last request the
/// source holds.
pub(crate) async fn wait_for_request(source: &ContextSource, taken: usize) -> Result<Request> {
    source.wait_for(taken + 1).await?.into_iter().last().or_fail()
}

/// The forwarded body read as JSON.
pub(crate) fn forwarded_body(request: &Request) -> Result<Value> {
    Ok(request.body_json::<Value>()?)
}

/// Every value of the `options` parameters of the forwarded request, in order.
pub(crate) fn forwarded_options(request: &Request) -> Vec<String> {
    request
        .url
        .query_pairs()
        .filter(|(key, _)| key == "options")
        .map(|(_, value)| value.into_owned())
        .collect()
}

/// The `@context` an answered entity leads with: the member itself when it is a string, or the first item of a list
/// of one or two, which the cases read alike.
pub(crate) fn leading_context(entity: &Value) -> Option<&str> {
    match entity.get("@context")? {
        Value::String(context) => Some(context),
        Value::Array(items) if (1..=2).contains(&items.len()) => items.first()?.as_str(),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::Array(_) | Value::Object(_) => None,
    }
}
