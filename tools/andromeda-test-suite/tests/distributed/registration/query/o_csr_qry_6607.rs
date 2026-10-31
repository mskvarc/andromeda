//! A malformed link header is bad request data.

use andromeda_test_suite::{broker, namespace::Namespace, problem::problem, raw_call::target, wire::request_to};
use googletest::prelude::*;
use http::{
    HeaderValue,
    Method,
    StatusCode,
    header::{ACCEPT, LINK},
};
use ngsi_ld::model::ErrorType;

/// Pins: Query Context Source Registrations by type with a Link header that is no link answers 400 `BadRequestData`
/// Clause: 5.10.2.4, 5.5.4, 6.3.5
/// Recorded: clause 6.3.5 does not say how a Link the broker cannot read is answered; Andromeda refuses it as
/// `BadRequestData`
#[gtest]
#[tokio::test]
async fn o_csr_qry_6607_a_malformed_link_header_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let type_text = namespace.suffixed("Vehicle");
    // A `Link` header that is no link is no header the typed client writes, so the request is sent as written.
    let request = request_to(&broker, Method::GET, &target("csourceRegistrations", &[("type", type_text.as_str())]))?
        .append_header(ACCEPT, HeaderValue::from_static("application/ld+json"))
        .append_header(LINK, HeaderValue::from_static("this is not a valid link header"));
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    Ok(())
}
