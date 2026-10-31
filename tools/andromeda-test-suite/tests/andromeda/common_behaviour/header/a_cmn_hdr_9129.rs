//! Forwarded fields from a peer outside the listed proxies are ignored.

use andromeda_test_suite::{
    broker,
    config::{BrokerRole, SuiteConfig},
    minted_context_url::{mint, remove},
    namespace::Namespace,
};
use googletest::prelude::*;

/// Pins: On a broker believing only proxies the request did not come from, Add @context with X-Forwarded-Host,
/// X-Forwarded-Proto or Forwarded naming another host and https stores the @context at a URL on the scheme and
/// authority the request addressed
/// Clause: 6.2, 5.13.2
/// Recorded: Andromeda extension, not v1.9.1: with http.forwarded.trust = "proxies", the forwarded fields of a peer
/// outside http.forwarded.proxies are ignored
#[gtest]
#[tokio::test]
async fn a_cmn_hdr_9129_forwarded_fields_from_a_peer_outside_the_listed_proxies_are_ignored() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker_edge_unlisted = broker::client(BrokerRole::EdgeUnlisted)?;
    let own_origin = SuiteConfig::get()?.broker(BrokerRole::EdgeUnlisted)?.clone();
    let requests = vec![
        vec![("X-Forwarded-Host", "attacker.example"), ("X-Forwarded-Proto", "https")],
        vec![("Forwarded", "proto=https;host=attacker.example")],
    ];
    for headers in requests {
        let minted = mint(&broker_edge_unlisted, &namespace, "forwardedHost", &headers).await?;
        verify_that!(&minted.url, eq(&minted.on(&own_origin)?)).and_log_failure_with_message(|| format!("under {headers:?}"));
        remove(&broker_edge_unlisted, &minted).await?;
    }
    Ok(())
}
