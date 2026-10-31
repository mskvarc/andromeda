//! Conformance mode refuses a dataset id on purge entities.

use andromeda_test_suite::{broker, config::BrokerRole, namespace::Namespace, problem::problem, wire::request_to};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::model::ErrorType;

/// Pins: a broker in conformance mode, with the purge dataset scope flag also on, refuses a datasetId on Purge
/// Entities as `InvalidRequest`
/// Clause: 5.6.21.3, 6.3.20, 6.4.3.3
/// Recorded: Andromeda extension, not v1.9.1: clause 5.6.21.3 gives Purge Entities no datasetId, and clause 6.3.20
/// answers a parameter incompatible with the operation with `InvalidRequest`; conformance mode forces every
/// extension off whatever its own flag says
#[gtest]
#[tokio::test]
async fn a_ext_adv_0003_conformance_mode_refuses_a_dataset_id_on_purge_entities() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker_conformance = broker::client(BrokerRole::Conformance)?;
    // Purge Entities defines no datasetId, so the parameter is written as the test needs it.
    let request = request_to(
        &broker_conformance,
        Method::DELETE,
        &format!("entities?type=Vehicle_{namespace}&drop=speed&datasetId=urn:ngsi-ld:Dataset:{namespace}-one"),
    )?;
    let answer = broker_conformance.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest));
    Ok(())
}
