//! Batch create reports a mixed arity entity in its own error.

use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    nth::nth,
    partial_outcome::reported,
    problem::problem,
    report_matcher::batch_error,
    unchecked_call::exchange,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::{BatchOperationResult, headers::Accept},
    model::{ErrorType, NgsiLdMediaType},
};

/// Pins: Batch Entity Creation reports an entity whose `LineString` mixes two- and three-ordinate positions as an
/// error of its own, creates the valid entity and stores the refused one nowhere
/// Clause: 4.7.1, 5.6.7, 5.6.7.4
/// Recorded: RFC 7946 section 3.1.1 constrains each position on its own, so it permits a geometry whose positions
/// differ in arity, and 4.7.1 does not forbid it, so the specification leaves the answer open; the user ruled on
/// 2026-10-06 that such a geometry must be rejected; no clause names the error type, so `BadRequestData` and
/// `InvalidRequest` are both accepted
#[gtest]
#[tokio::test]
async fn o_qry_geo_2303_batch_create_reports_a_mixed_arity_entity_in_its_own_error() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let good_id = namespace.urn("Building", "good")?;
    let bad_id = namespace.urn("Building", "bad")?;
    // The model refuses a LineString whose positions differ in arity, so the batch goes out as written.
    let batch = format!(
        r#"[{{"id": "{good_id}", "type": "Building_{namespace}", "location": {{"type": "GeoProperty", "value": {{"type": "LineString", "coordinates": [[13.4, 52.52], [13.5, 52.53]]}}}}}}, {{"id": "{bad_id}", "type": "Building_{namespace}", "location": {{"type": "GeoProperty", "value": {{"type": "LineString", "coordinates": [[13.4, 52.52], [13.5, 52.53, 250.0]]}}}}}}]"#
    );
    let request = exchange(
        &broker,
        Method::POST,
        "entityOperations/create",
        &[("Content-Type", "application/json")],
        Some(batch.as_str()),
    )?;
    let answer = broker.send_unchecked(request).await;
    let report: BatchOperationResult = reported(answer)?;
    expect_that!(&report.success, all!(len(eq(1)), nth(0, eq(&good_id))));
    expect_that!(
        &report.errors,
        all!(
            len(eq(1)),
            nth(
                0,
                any!(
                    batch_error().entity(&bad_id).problem(ErrorType::BadRequestData),
                    batch_error().entity(&bad_id).problem(ErrorType::InvalidRequest)
                )
            )
        )
    );
    let missing = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&bad_id)
        .send()
        .await;
    expect_that!(&missing, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    Ok(())
}
