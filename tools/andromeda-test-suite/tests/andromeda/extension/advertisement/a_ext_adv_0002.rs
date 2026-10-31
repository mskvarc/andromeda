//! Conformance mode leaves the temporal delete by query unrouted.

use andromeda_test_suite::{broker, config::BrokerRole, namespace::Namespace, problem::answered, wire::request_to};
use googletest::prelude::*;
use http::{Method, StatusCode};

/// Pins: a broker in conformance mode, with the temporal delete-by-query flag also on, answers DELETE on the
/// temporal entities collection with 405, as a broker without the extension does
/// Clause: 6.18.3
/// Recorded: Andromeda extension, not v1.9.1: clause 6.18.3 binds only GET and POST to the temporal entities
/// collection; conformance mode forces every extension off whatever its own flag says
#[gtest]
#[tokio::test]
async fn a_ext_adv_0002_conformance_mode_leaves_the_temporal_delete_by_query_unrouted() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker_conformance = broker::client(BrokerRole::Conformance)?;
    // No operation of the specification deletes temporal evolutions by a query, so the request is sent as written.
    let request = request_to(
        &broker_conformance,
        Method::DELETE,
        &format!("temporal/entities?type=Vehicle_{namespace}&drop=speed"),
    )?;
    let answer = broker_conformance.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::METHOD_NOT_ALLOWED));
    Ok(())
}
