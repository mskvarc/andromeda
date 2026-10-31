//! A malformed forwarded field does not fail the request.

use andromeda_test_suite::{
    broker,
    minted_context_url::{mint, remove},
    namespace::Namespace,
};
use googletest::prelude::*;

/// The requests of the test: the forwarded fields each carries.
const REQUESTS: [&[(&str, &str)]; 5] = [
    &[("Forwarded", "definitely not rfc 7239")],
    &[("Forwarded", ";;;")],
    &[("Forwarded", "proto=gopher")],
    &[("X-Forwarded-Proto", "not-a-scheme")],
    &[("X-Forwarded-Proto", "ws")],
];

/// Pins: Add @context with a Forwarded field that is no RFC 7239 element, names an empty element or a third scheme,
/// or an X-Forwarded-Proto naming no http scheme, answers 201 and stores the @context at an http URL
/// Clause: 6.2, 5.13.2
#[gtest]
#[tokio::test]
async fn a_cmn_hdr_9002_a_malformed_forwarded_field_does_not_fail_the_request() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    for headers in REQUESTS {
        let minted = mint(&broker, &namespace, "forwardedProto", headers).await?;
        verify_that!(minted.url.as_str(), starts_with("http://")).and_log_failure_with_message(|| format!("under {headers:?}"));
        verify_that!(minted.url.as_str(), ends_with(minted.location.as_str())).and_log_failure_with_message(|| format!("under {headers:?}"));
        remove(&broker, &minted).await?;
    }
    Ok(())
}
