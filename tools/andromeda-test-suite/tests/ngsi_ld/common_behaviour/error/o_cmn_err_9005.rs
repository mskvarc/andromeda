//! A request body that stops arriving gets a 408 problem document once the request deadline passes.

use andromeda_test_suite::{config::BrokerRole, raw_http::send_bytes};
use googletest::prelude::*;
use http::{Method, StatusCode, header::CONNECTION};
use iri_rs::IriRefBuf;
use ngsi_ld::model::{ErrorType, ProblemDetails};

/// Pins: a `POST /entities` whose `Content-Length` announces more than the client sends is answered, once the
/// broker's request deadline passes, 408 with a `RequestTimeout` problem document naming the request target and with
/// `Connection: close`, never 500
/// Clause: 5.5.3, 6.3.2
/// Recorded: the client failed to send its request in time, which IETF RFC 9110 clause 15.5.9 answers 408 with the
/// connection closed; Table 6.3.2-1 names no error type for 408, so `RequestTimeout` is Andromeda's own, the same
/// move `MethodNotAllowed` is for 405
#[gtest]
#[tokio::test]
async fn o_cmn_err_9005_a_request_body_that_stops_arriving_gets_a_408_problem_document() -> Result<()> {
    let request_timeout = "https://uri.etsi.org/ngsi-ld/errors/RequestTimeout".parse::<ErrorType>()?;
    // The HTTP client sends the whole body it announces, so the short body is written to the socket.
    let answer = send_bytes(
        BrokerRole::Front,
        &Method::POST,
        "entities",
        &[("Content-Type", "application/json"), ("Content-Length", "64")],
        b"{",
    )
    .await?;
    expect_that!(answer.status(), eq(StatusCode::REQUEST_TIMEOUT));
    expect_that!(answer.headers().get(CONNECTION).and_then(|value| value.to_str().ok()), some(eq("close")));
    let details = serde_json::from_slice::<ProblemDetails>(answer.body())?;
    expect_that!(details.error_type, some(eq(&request_timeout)));
    expect_that!(details.title.as_deref(), some(eq("Request timeout")));
    expect_that!(details.status, some(eq(408)));
    expect_that!(details.detail.as_deref(), some(contains_substring("request body did not arrive")));
    expect_that!(details.instance.as_ref().map(IriRefBuf::as_str), some(eq("/ngsi-ld/v1/entities")));
    Ok(())
}
