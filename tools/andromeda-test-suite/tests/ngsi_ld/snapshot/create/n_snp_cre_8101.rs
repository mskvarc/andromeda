//! Output members a client sends are ignored on create and update.

use andromeda_test_suite::{broker, eventually::retried, namespace::Namespace, problem::answered};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{
        UncheckedDocument,
        headers::Accept,
        operation::{CreateSnapshot, UpdateSnapshot},
    },
    model::{DateTime, NgsiLdMediaType, RawJson},
    snapshot::{ExecutionStatus, QueryDocument, Snapshot, SnapshotFragment, SnapshotId, SnapshotPriority, SnapshotStatus},
};

/// Pins: Create Snapshot and Update Snapshot Status ignore the output-only members a client sends
/// (`snapshotStatus`, the execution details and the four timestamps): the Snapshot reports the status and details
/// of its own execution and timestamps of its own, and the update still applies its `snapshotPriority`
/// Clause: Table 5.2.41-2, 5.16.1.4, 5.16.4.4
#[gtest]
#[tokio::test]
async fn n_snp_cre_8101_output_members_a_client_sends_are_ignored_on_create_and_update() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let snapshot_id = format!("urn:ngsi-ld:Snapshot:{namespace}-1").parse::<SnapshotId>()?;
    let snapshot = Snapshot::builder()
        .id(snapshot_id.clone())
        .query(QueryDocument::builder().entities([vehicle_type.as_str()]).build()?)
        .build()?;
    // The output-only members are no members of a typed request, so they are written as text beside it.
    let output_members: [(&str, &str); 7] = [
        ("snapshotStatus", r#""success""#),
        ("snapshotQueriesDetails", r#"[{"resultStatus": "failure"}]"#),
        ("snapshotTemporalQueriesDetails", r#"[{"resultStatus": "failure"}]"#),
        ("createdAt", r#""1900-01-01T00:00:00Z""#),
        ("modifiedAt", r#""1900-01-01T00:00:00Z""#),
        ("expiresAt", r#""1900-01-01T00:00:00Z""#),
        ("lastUsedAt", r#""1900-01-01T00:00:00Z""#),
    ];
    let document = output_members.iter().try_fold(
        UncheckedDocument::try_from(snapshot.render())?,
        |document, (name, json)| -> Result<UncheckedDocument> {
            let member: RawJson = json.parse()?;
            Ok(document.set_member(name, &member))
        },
    )?;
    let request = broker.unchecked(&CreateSnapshot { snapshot: &snapshot })?.document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    let epoch = "1900-01-01T00:00:00Z".parse::<DateTime>()?;
    retried(async || {
        let retrieved = broker
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .snapshots()
            .retrieve(&snapshot_id)
            .send()
            .await?;
        verify_that!(retrieved.status(), eq("empty".parse::<SnapshotStatus>()?))?;
        verify_that!(retrieved.output().queries_details, len(eq(1)))?;
        verify_that!(
            retrieved.output().queries_details.first().map(|details| details.result_status),
            some(eq(ExecutionStatus::Empty))
        )?;
        verify_that!(retrieved.output().temporal_queries_details, is_empty())?;
        verify_that!(retrieved.output().created_at, some(not(eq(epoch))))?;
        verify_that!(retrieved.output().modified_at, some(not(eq(epoch))))?;
        verify_that!(retrieved.output().expires_at, not(eq(epoch)))?;
        verify_that!(retrieved.output().last_used_at, some(not(eq(epoch))))?;
        Ok(())
    })
    .await?;
    let fragment = SnapshotFragment::builder().priority(7).build()?;
    // The update also sends output-only members, which no typed fragment writes, so they are written as text.
    let status: RawJson = r#""failure""#.parse()?;
    let created_at: RawJson = r#""1900-01-01T00:00:00Z""#.parse()?;
    let update_document = UncheckedDocument::try_from(fragment.render())?
        .set_member("snapshotStatus", &status)
        .set_member("createdAt", &created_at)
        .set_member("expiresAt", &created_at);
    let update = broker
        .unchecked(&UpdateSnapshot {
            id: &snapshot_id,
            fragment: &fragment,
        })?
        .document(&update_document);
    let updated = broker.send_unchecked(update).await;
    expect_that!(&updated, answered(StatusCode::OK));
    let after = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .snapshots()
        .retrieve(&snapshot_id)
        .send()
        .await?;
    expect_that!(after.priority(), some(eq(SnapshotPriority::try_from(7)?)));
    expect_that!(after.status(), eq("empty".parse::<SnapshotStatus>()?));
    expect_that!(after.output().created_at, some(not(eq(epoch))));
    expect_that!(after.output().expires_at, not(eq(epoch)));
    broker.snapshots().delete(&snapshot_id).send().await?;
    Ok(())
}
