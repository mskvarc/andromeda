//! The terminator redirects a plaintext client to its TLS listener.

use andromeda_test_suite::{broker, config::BrokerRole, problem::answered, wire::request_to};
use googletest::prelude::*;
use http::{Method, StatusCode, header::LOCATION};

/// Pins: a plaintext request to the terminator's plaintext listener is answered 301 with a Location naming the same
/// target on the terminator's TLS listener, and is not served
/// Clause: 6.2
/// Recorded: clause 6.2 is met either by the broker terminating TLS itself or by a proxy in front of it; this
/// broker serves plaintext behind a TLS terminator and believes the forwarded fields the terminator sets; clause 6.2 does not recommend HTTP without TLS, so the terminator sends a plaintext client to TLS
#[gtest]
#[tokio::test]
async fn a_tls_trm_0003_the_terminator_redirects_a_plaintext_client_to_its_tls_listener() -> Result<()> {
    let plaintext = broker::client(BrokerRole::TerminatorPlaintext)?;
    let answer = plaintext.send_unchecked(request_to(&plaintext, Method::GET, "info/sourceIdentity")?).await;
    expect_that!(
        &answer,
        answered(StatusCode::MOVED_PERMANENTLY).with_header(LOCATION, "https://terminator:8443/ngsi-ld/v1/info/sourceIdentity")
    );
    Ok(())
}
