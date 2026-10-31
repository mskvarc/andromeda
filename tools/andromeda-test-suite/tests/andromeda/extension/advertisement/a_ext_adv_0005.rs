//! Conformance mode refuses a join on the temporal query.

use andromeda_test_suite::{broker, config::BrokerRole, namespace::Namespace, problem::problem, wire::request_to};
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::ACCEPT};
use ngsi_ld::{
    http::headers::Accept,
    model::{ErrorType, NgsiLdMediaType},
};

/// Pins: a broker in conformance mode, with the temporal linked retrieval flag also on, refuses join on Query
/// Temporal Evolution of Entities as `InvalidRequest`
/// Clause: 5.7.4.3, 6.3.20, 6.18.3.2
/// Recorded: Andromeda extension, not v1.9.1: clause 5.7.4.3 gives the temporal query no join, and clause 6.3.20
/// answers a parameter incompatible with the operation with `InvalidRequest`; conformance mode forces every
/// extension off whatever its own flag says
#[gtest]
#[tokio::test]
async fn a_ext_adv_0005_conformance_mode_refuses_a_join_on_the_temporal_query() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker_conformance = broker::client(BrokerRole::Conformance)?;
    // The client cannot write a parameter of this read, so it is sent as written.
    let request = request_to(
        &broker_conformance.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        &format!("temporal/entities?type=Vehicle_{namespace}&timerel=after&timeAt=1970-01-01T00%3A00%3A00Z&join=flat"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer = broker_conformance.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest));
    Ok(())
}
