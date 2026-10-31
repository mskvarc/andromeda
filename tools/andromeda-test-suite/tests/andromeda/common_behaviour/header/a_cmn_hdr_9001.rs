//! Forwarded protocol fields are ignored without a trusted proxy.

use andromeda_test_suite::{
    broker,
    minted_context_url::{mint, remove},
    namespace::Namespace,
};
use googletest::prelude::*;

/// The requests of the test: the forwarded fields each carries, in the order sent.
const REQUESTS: [&[(&str, &str)]; 9] = [
    &[("X-Forwarded-Proto", "https")],
    &[("Forwarded", "proto=https")],
    &[("Forwarded", "proto=https"), ("X-Forwarded-Proto", "https")],
    &[("Forwarded", "proto=\"https\"")],
    &[("Forwarded", "for=192.0.2.60;proto=https;host=terminator.example")],
    &[("Forwarded", "for=192.0.2.60"), ("X-Forwarded-Proto", "https")],
    &[("Forwarded", "for=192.0.2.60")],
    &[("X-Forwarded-Proto", "http")],
    &[("Forwarded", "proto=http")],
];

/// Pins: Add @context with X-Forwarded-Proto or Forwarded naming https, alone, together, quoted or in a full
/// element, stores the @context at an http URL when the broker was not told to trust its edge
/// Clause: 6.2, 5.13.2
#[gtest]
#[tokio::test]
async fn a_cmn_hdr_9001_forwarded_protocol_fields_are_ignored_without_a_trusted_proxy() -> Result<()> {
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
