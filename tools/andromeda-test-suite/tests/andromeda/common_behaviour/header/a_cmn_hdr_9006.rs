//! At a trusted edge the leftmost forwarded element decides.

use andromeda_test_suite::{
    broker,
    config::{BrokerRole, SuiteConfig},
    minted_context_url::{mint, remove},
    namespace::Namespace,
};
use googletest::prelude::*;

/// Pins: On a broker trusting its edge, Add @context with a Forwarded or X-Forwarded-Proto field listing two
/// protocols stores its @context under the leftmost one
/// Clause: 6.2, 5.13.2
/// Recorded: Andromeda extension, not v1.9.1: from a peer `http.forwarded.trust` believes, the scheme of the apiRoot
/// it mints comes from the Forwarded (IETF RFC 7239) or X-Forwarded-Proto field, and the authority from the host of
/// Forwarded or X-Forwarded-Host, else the request's own
#[gtest]
#[tokio::test]
async fn a_cmn_hdr_9006_at_a_trusted_edge_the_leftmost_forwarded_element_decides() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker_edge = broker::client(BrokerRole::Edge)?;
    let config = SuiteConfig::get()?;
    let http_origin = config.broker(BrokerRole::Edge)?.clone();
    let https_origin = config.broker(BrokerRole::EdgeHttps)?.clone();
    let requests = vec![
        (vec![("Forwarded", "proto=https,proto=http")], &https_origin),
        (vec![("Forwarded", "proto=http,proto=https")], &http_origin),
        (vec![("X-Forwarded-Proto", "https, http")], &https_origin),
        (vec![("X-Forwarded-Proto", "http, https")], &http_origin),
    ];
    for (headers, origin) in requests {
        let minted = mint(&broker_edge, &namespace, "forwardedProto", &headers).await?;
        verify_that!(&minted.url, eq(&minted.on(origin)?)).and_log_failure_with_message(|| format!("under {headers:?}"));
        remove(&broker_edge, &minted).await?;
    }
    Ok(())
}
