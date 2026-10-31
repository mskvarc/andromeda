//! A snapshot header that is not a uri or names no snapshot is refused.

use andromeda_test_suite::{broker, namespace::Namespace, problem::problem, raw_call::target, unchecked_call::exchange};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::model::ErrorType;

/// Pins: Query Entities with an empty NGSILD-Snapshot, one that is not a URI and one holding a space answers 400
/// `BadRequestData`, and one naming no Snapshot answers 404 `ResourceNotFound`
/// Clause: 6.3.22, 5.5.15
/// Recorded: clause 6.3.22 names no error for an NGSILD-Snapshot value that is not a URI or that names no Snapshot;
/// Andromeda answers `BadRequestData` and `ResourceNotFound`
#[gtest]
#[tokio::test]
async fn o_snp_qry_8112_a_snapshot_header_that_is_not_a_uri_or_names_no_snapshot_is_refused() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let query = target("entities", &[("type", vehicle_type.as_str())]);
    // The typed header cannot hold a value that is no snapshot identifier, so each request is sent as written.
    for value in ["", "not a URI", &format!("urn:ngsi-ld:Snapshot:{namespace} header")] {
        let answer = broker
            .send_unchecked(exchange(&broker, Method::GET, &query, &[("NGSILD-Snapshot", value)], None)?)
            .await;
        expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    }
    let unknown_header = format!("urn:ngsi-ld:Snapshot:{namespace}-unknown");
    let unknown = broker
        .send_unchecked(exchange(&broker, Method::GET, &query, &[("NGSILD-Snapshot", unknown_header.as_str())], None)?)
        .await;
    expect_that!(&unknown, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    Ok(())
}
