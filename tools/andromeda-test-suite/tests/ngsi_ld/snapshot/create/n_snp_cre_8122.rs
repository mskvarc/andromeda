//! A body that is not json is invalid request and a wrong snapshot bad request data.

use andromeda_test_suite::{broker, namespace::Namespace, problem::problem, written_json::json_request_to};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::model::ErrorType;

/// Pins: Create Snapshot answers 400 `InvalidRequest` for a body that is not JSON and 400 `BadRequestData` for a
/// JSON body whose type is not Snapshot
/// Clause: 6.3.2, 5.5.4, 5.16.1.4
///
/// Invalid type: Entity (a body whose type names no Snapshot)
#[gtest]
#[tokio::test]
async fn n_snp_cre_8122_a_body_that_is_not_json_is_invalid_request_and_a_wrong_snapshot_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    // A body that is not JSON and a type that is no Snapshot are what the typed builders cannot write, so both are written as text.
    let request = json_request_to(&broker, Method::POST, "snapshots", "{")?;
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest));
    let text = format!(
        r#"{{"id": "urn:ngsi-ld:Snapshot:{namespace}-1", "type": "Entity", "snapshotQueries": [{{"type": "Query", "entities": [{{"type": "Vehicle_{namespace}"}}]}}]}}"#
    );
    let request_2 = json_request_to(&broker, Method::POST, "snapshots", &text)?;
    let answer_2 = broker.send_unchecked(request_2).await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    Ok(())
}
