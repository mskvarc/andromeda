//! A wrong method on the management listener gets a 405 problem document.

use andromeda_test_suite::{
    config::{BrokerRole, SuiteConfig},
    problem::problem,
    tls_connection::Connection,
    wire::request_to,
};
use googletest::prelude::*;
use http::{Method, StatusCode, header::ALLOW};
use ngsi_ld::{client::ClientError, model::ErrorType};

/// Pins: a wrong-method request on the management listener is answered 405 with a `MethodNotAllowed` problem document
/// and an Allow header naming what the probe serves
/// Clause: 5.5.3, 6.3.2
/// Recorded: the management listener is Andromeda's own and shares the broker's TLS identity (clause 6.2); Table 6.3.2-1
/// lists 405 among the HTTP binding errors but names no error type for it, so `MethodNotAllowed` is Andromeda's own
#[gtest]
#[tokio::test]
async fn a_tls_tls_0009_a_wrong_method_on_the_management_listener_gets_a_405_problem_document() -> Result<()> {
    let management = Connection::new().trusting("tls/ca.crt").client(BrokerRole::TlsManagement).await?;
    // The readiness probe sits at the root of the management listener, outside the API root.
    let probe = SuiteConfig::get()?.broker(BrokerRole::TlsManagement)?.join("readyz")?;
    let answer = management
        .send_unchecked(request_to(&management, Method::POST, "")?.uri(probe.as_str().parse()?))
        .await;
    expect_that!(
        &answer,
        problem(
            StatusCode::METHOD_NOT_ALLOWED,
            "https://uri.etsi.org/ngsi-ld/errors/MethodNotAllowed".parse::<ErrorType>()?
        )
        .with_header_containing(ALLOW, "GET")
    );
    let details = answer.as_ref().err().and_then(ClientError::problem).or_fail()?;
    expect_that!(details.status, some(eq(405)));
    expect_that!(details.instance.as_ref().map(ToString::to_string), some(eq("/readyz")));
    Ok(())
}
