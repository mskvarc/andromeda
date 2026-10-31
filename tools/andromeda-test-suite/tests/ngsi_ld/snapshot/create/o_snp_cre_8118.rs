//! A snapshot with a member outside its data type is refused.

use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    problem::{answered, problem},
    written_json::json_request_to,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::headers::Accept,
    model::{ErrorType, NgsiLdMediaType},
    snapshot::SnapshotId,
};

/// Pins: Create Snapshot with a member the Snapshot data type does not define answers 400 `BadRequestData` and
/// creates no Snapshot
/// Clause: 5.16.1.4, 5.2.41
/// Recorded: Table 5.2.41-1 lists the members of a Snapshot and says nothing of others; Andromeda refuses a member
/// it does not define
#[gtest]
#[tokio::test]
async fn o_snp_cre_8118_a_snapshot_with_a_member_outside_its_data_type_is_refused() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let snapshot_id = format!("urn:ngsi-ld:Snapshot:{namespace}-1").parse::<SnapshotId>()?;
    // The member is outside the Snapshot data type, which the typed builder cannot write, so it is written as text.
    let text = format!(
        r#"{{"id": "{snapshot_id}", "type": "Snapshot", "snapshotQueries": [{{"type": "Query", "entities": [{{"type": "Vehicle_{namespace}"}}]}}], "unknown": true}}"#
    );
    let request = json_request_to(&broker, Method::POST, "snapshots", &text)?;
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let missing = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .snapshots()
        .retrieve(&snapshot_id)
        .send()
        .await;
    expect_that!(&missing, answered(StatusCode::NOT_FOUND));
    Ok(())
}
