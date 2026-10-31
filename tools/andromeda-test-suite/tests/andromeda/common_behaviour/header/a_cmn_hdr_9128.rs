//! A believed edge mints the host the forwarded fields name.

use andromeda_test_suite::{
    broker,
    config::{BrokerRole, SuiteConfig},
    minted_context_url::{mint, remove},
    namespace::Namespace,
};
use googletest::prelude::*;
use url::Url;

/// Pins: On a broker believing its edge, Add @context stores its @context at a URL whose authority is the host
/// X-Forwarded-Host or the host of Forwarded names, its port included, quoted or bare, the leftmost one of a list,
/// Forwarded outranking X-Forwarded-Host and a host-less Forwarded falling through to X-Forwarded-Host, and the
/// request's own authority when the forwarded host is unusable
/// Clause: 6.2, 5.13.2
/// Recorded: Andromeda extension, not v1.9.1: from a peer `http.forwarded.trust` believes, the scheme of the apiRoot
/// it mints comes from the Forwarded (IETF RFC 7239) or X-Forwarded-Proto field, and the authority from the host of
/// Forwarded or X-Forwarded-Host, else the request's own
#[gtest]
#[tokio::test]
async fn a_cmn_hdr_9128_a_believed_edge_mints_the_host_the_forwarded_fields_name() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker_edge = broker::client(BrokerRole::Edge)?;
    let own_origin = SuiteConfig::get()?.broker(BrokerRole::Edge)?.clone();
    let public_port = Url::parse("http://public.example:8443")?;
    let public_https = Url::parse("https://public.example")?;
    let public_https_port = Url::parse("https://public.example:8443")?;
    let public = Url::parse("http://public.example")?;
    let rfc = Url::parse("http://rfc.example")?;
    let de_facto_https = Url::parse("https://de-facto.example")?;
    let requests = vec![
        (vec![("X-Forwarded-Host", "public.example:8443")], &public_port),
        (vec![("X-Forwarded-Host", "public.example"), ("X-Forwarded-Proto", "https")], &public_https),
        (vec![("Forwarded", "proto=https;host=\"public.example:8443\"")], &public_https_port),
        (vec![("Forwarded", "host=public.example, host=internal.example")], &public),
        (vec![("X-Forwarded-Host", "public.example, internal.example")], &public),
        (vec![("Forwarded", "host=rfc.example"), ("X-Forwarded-Host", "de-facto.example")], &rfc),
        (
            vec![("Forwarded", "for=192.0.2.60;proto=https"), ("X-Forwarded-Host", "de-facto.example")],
            &de_facto_https,
        ),
        (vec![("X-Forwarded-Host", "user@public.example")], &own_origin),
        (vec![("X-Forwarded-Host", "public.example:99999")], &own_origin),
    ];
    for (headers, origin) in requests {
        let minted = mint(&broker_edge, &namespace, "forwardedHost", &headers).await?;
        verify_that!(&minted.url, eq(&minted.on(origin)?)).and_log_failure_with_message(|| format!("under {headers:?}"));
        remove(&broker_edge, &minted).await?;
    }
    Ok(())
}
