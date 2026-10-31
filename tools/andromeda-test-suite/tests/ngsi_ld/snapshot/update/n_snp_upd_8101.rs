//! Update snapshot status refuses read only members.

use andromeda_test_suite::{
    broker,
    broker::BrokerClient,
    namespace::Namespace,
    problem::{answered, problem, problem_status},
    snapshot_capture::await_status,
    written_json::json_request_to,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::headers::Accept,
    model::{ErrorType, NgsiLdMediaType, Output},
    snapshot::{QueryDocument, SettledStatus, Snapshot, SnapshotId, SnapshotPriority, SnapshotStatus},
};
use std::str;

/// Patches the Snapshot `id` with `fragment` as written and expects the 400 `BadRequestData` refusal.
async fn expect_refused(broker: &BrokerClient, id: &SnapshotId, fragment: &str) -> Result<()> {
    let answer = broker
        .send_unchecked(json_request_to(broker, Method::PATCH, &format!("snapshots/{id}"), fragment)?)
        .await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    expect_that!(problem_status(&answer), some(eq(400)));
    Ok(())
}

/// Pins: Update Snapshot Status answers 400 `BadRequestData` for a fragment changing the id, setting the type to
/// Entity or null, or naming `snapshotQueries` or `snapshotTemporalQueries`, and leaves the Snapshot unchanged; a
/// fragment with output members beside `snapshotPriority` answers 200 with only the priority applied
/// Clause: 5.16.4.4, Table 5.2.41-1, Table 5.2.41-2
/// Invalid type: Entity (a body whose type names no Snapshot)
#[gtest]
#[tokio::test]
async fn n_snp_upd_8101_update_snapshot_status_refuses_read_only_members() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let snapshot_id = format!("urn:ngsi-ld:Snapshot:{namespace}-1").parse::<SnapshotId>()?;
    let different_id = format!("urn:ngsi-ld:Snapshot:{namespace}-different").parse::<SnapshotId>()?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let snapshot = Snapshot::builder()
        .id(snapshot_id.clone())
        .query(QueryDocument::builder().entities([vehicle_type.as_str()]).build()?)
        .build()?;
    broker.snapshots().create(&snapshot).send().await?;
    await_status(&broker, &snapshot_id, SnapshotStatus::Settled(SettledStatus::Empty)).await?;
    let json = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    let created = json.snapshots().retrieve(&snapshot_id).send().await?;
    let created_at = created.output().created_at.or_fail()?;
    // The fragment model writes only members a client may change, so each refused fragment is sent as written.
    expect_refused(&broker, &snapshot_id, &format!(r#"{{"id": "urn:ngsi-ld:Snapshot:{namespace}-different"}}"#)).await?;
    expect_refused(&broker, &snapshot_id, r#"{"type": "Entity"}"#).await?;
    expect_refused(&broker, &snapshot_id, r#"{"type": null}"#).await?;
    expect_refused(&broker, &snapshot_id, r#"{"snapshotQueries": []}"#).await?;
    expect_refused(&broker, &snapshot_id, r#"{"snapshotTemporalQueries": []}"#).await?;
    let unchanged = json.snapshots().retrieve(&snapshot_id).send().await?;
    expect_that!(unchanged.queries(), some(len(eq(1))));
    expect_that!(unchanged.temporal_queries(), none());
    let absent = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .snapshots()
        .retrieve(&different_id)
        .send()
        .await;
    expect_that!(&absent, answered(StatusCode::NOT_FOUND));
    // Output members beside the priority are no fragment the model writes, so the request is sent as written.
    let applied = broker
        .send_unchecked(json_request_to(
            &broker,
            Method::PATCH,
            &format!("snapshots/{snapshot_id}"),
            r#"{"snapshotPriority": 6, "snapshotStatus": "success", "createdAt": "1900-01-01T00:00:00Z"}"#,
        )?)
        .await?;
    expect_that!(applied.status(), eq(StatusCode::OK));
    let updated = Snapshot::<Output>::parse(str::from_utf8(applied.body())?)?;
    expect_that!(updated.priority(), some(eq(SnapshotPriority::try_new(6)?)));
    expect_that!(updated.status(), eq(SnapshotStatus::Settled(SettledStatus::Empty)));
    expect_that!(updated.output().created_at, some(eq(created_at)));
    let retrieved = json.snapshots().retrieve(&snapshot_id).send().await?;
    expect_that!(retrieved.priority(), some(eq(SnapshotPriority::try_new(6)?)));
    broker.snapshots().delete(&snapshot_id).send().await?;
    Ok(())
}
