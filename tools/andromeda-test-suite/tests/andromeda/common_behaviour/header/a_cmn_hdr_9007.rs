//! At a trusted edge a malformed forwarded field leaves http.

use andromeda_test_suite::{
    broker,
    config::{BrokerRole, SuiteConfig},
    minted_context_url::{mint, remove},
    namespace::Namespace,
};
use googletest::prelude::*;

/// Pins: On a broker trusting its edge, Add @context with a Forwarded field that is no RFC 7239 element, names an
/// empty element or a third scheme, or an X-Forwarded-Proto naming no http scheme, answers 201 and stores the
/// @context at an http URL
/// Clause: 6.2, 5.13.2
/// Recorded: Andromeda extension, not v1.9.1: from a peer `http.forwarded.trust` believes, the scheme of the apiRoot
/// it mints comes from the Forwarded (IETF RFC 7239) or X-Forwarded-Proto field, and the authority from the host of
/// Forwarded or X-Forwarded-Host, else the request's own
#[gtest]
#[tokio::test]
async fn a_cmn_hdr_9007_at_a_trusted_edge_a_malformed_forwarded_field_leaves_http() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker_edge = broker::client(BrokerRole::Edge)?;
    let config = SuiteConfig::get()?;
    let http_origin = config.broker(BrokerRole::Edge)?.clone();
    let requests = vec![
        (vec![("Forwarded", "definitely not rfc 7239")], &http_origin),
        (vec![("Forwarded", ";;;")], &http_origin),
        (vec![("Forwarded", "proto=gopher")], &http_origin),
        (vec![("X-Forwarded-Proto", "not-a-scheme")], &http_origin),
        (vec![("X-Forwarded-Proto", "ws")], &http_origin),
    ];
    for (headers, origin) in requests {
        let minted = mint(&broker_edge, &namespace, "forwardedProto", &headers).await?;
        verify_that!(&minted.url, eq(&minted.on(origin)?)).and_log_failure_with_message(|| format!("under {headers:?}"));
        remove(&broker_edge, &minted).await?;
    }
    Ok(())
}
