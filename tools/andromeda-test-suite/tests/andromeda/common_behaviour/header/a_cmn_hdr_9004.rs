//! A trusted edge mints the scheme the forwarded fields name.

use andromeda_test_suite::{
    broker,
    config::{BrokerRole, SuiteConfig},
    minted_context_url::{mint, remove},
    namespace::Namespace,
};
use googletest::prelude::*;
use url::Url;

/// Pins: On a broker trusting its edge, Add @context stores its @context at a URL whose scheme is the one
/// X-Forwarded-Proto or the proto of Forwarded names, quoted or bare, beside other parameters (whose host then names
/// the authority) or alone, falling through a for-only Forwarded element to X-Forwarded-Proto and keeping http when
/// http is forwarded or no protocol is named
/// Clause: 6.2, 5.13.2
/// Recorded: Andromeda extension, not v1.9.1: from a peer `http.forwarded.trust` believes, the scheme of the apiRoot
/// it mints comes from the Forwarded (IETF RFC 7239) or X-Forwarded-Proto field, and the authority from the host of
/// Forwarded or X-Forwarded-Host, else the request's own
#[gtest]
#[tokio::test]
async fn a_cmn_hdr_9004_a_trusted_edge_mints_the_scheme_the_forwarded_fields_name() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker_edge = broker::client(BrokerRole::Edge)?;
    let config = SuiteConfig::get()?;
    let http_origin = config.broker(BrokerRole::Edge)?.clone();
    let https_origin = config.broker(BrokerRole::EdgeHttps)?.clone();
    let terminator = Url::parse("https://terminator.example")?;
    let requests = vec![
        (vec![("X-Forwarded-Proto", "https")], &https_origin),
        (vec![("Forwarded", "proto=https")], &https_origin),
        (vec![("Forwarded", "proto=https"), ("X-Forwarded-Proto", "https")], &https_origin),
        (vec![("Forwarded", "proto=\"https\"")], &https_origin),
        (vec![("Forwarded", "for=192.0.2.60;proto=https;host=terminator.example")], &terminator),
        (vec![("Forwarded", "for=192.0.2.60"), ("X-Forwarded-Proto", "https")], &https_origin),
        (vec![("Forwarded", "for=192.0.2.60")], &http_origin),
        (vec![("X-Forwarded-Proto", "http")], &http_origin),
        (vec![("Forwarded", "proto=http")], &http_origin),
    ];
    for (headers, origin) in requests {
        let minted = mint(&broker_edge, &namespace, "forwardedProto", &headers).await?;
        verify_that!(&minted.url, eq(&minted.on(origin)?)).and_log_failure_with_message(|| format!("under {headers:?}"));
        remove(&broker_edge, &minted).await?;
    }
    Ok(())
}
