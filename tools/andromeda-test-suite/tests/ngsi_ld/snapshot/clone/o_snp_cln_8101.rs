//! A clone body with execution details is refused.

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
    snapshot::{QueryDocument, Snapshot, SnapshotId},
};

/// Pins: Clone Snapshot with a body carrying `snapshotQueriesDetails` answers 400 `BadRequestData` and creates no
/// clone
/// Clause: 5.16.2.3, 5.16.2.4
/// Recorded: clause 5.16.2.3 takes "a JSON-LD document conforming to the Snapshot data type ... but without
/// `snapshotQueriesDetails`", while Table 5.2.41-2 has output members provided on a create or an update ignored;
/// Andromeda refuses them in a clone body
#[gtest]
#[tokio::test]
async fn o_snp_cln_8101_a_clone_body_with_execution_details_is_refused() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let source_id = format!("urn:ngsi-ld:Snapshot:{namespace}-1").parse::<SnapshotId>()?;
    let clone_id = format!("urn:ngsi-ld:Snapshot:{namespace}-2").parse::<SnapshotId>()?;
    let snapshot = Snapshot::builder()
        .id(source_id.clone())
        .query(QueryDocument::builder().entities([vehicle_type.as_str()]).build()?)
        .build()?;
    broker.snapshots().create(&snapshot).send().await?;
    // A clone body carrying execution details is no typed `SnapshotClone`, so it is written as text.
    let text = format!(r#"{{"id": "{clone_id}", "type": "Snapshot", "snapshotQueriesDetails": []}}"#);
    let request = json_request_to(&broker, Method::POST, &format!("snapshots/{source_id}/clone"), &text)?;
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let missing = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .snapshots()
        .retrieve(&clone_id)
        .send()
        .await;
    expect_that!(&missing, answered(StatusCode::NOT_FOUND));
    broker.snapshots().delete(&source_id).send().await?;
    Ok(())
}
