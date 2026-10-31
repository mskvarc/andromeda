//! Concurrent clones under one id create one snapshot and a racing update delete and purge leave it gone.

use andromeda_test_suite::{
    answered_status::AnsweredStatus,
    broker,
    namespace::Namespace,
    problem::{answered, problem},
    snapshot_capture::await_status,
};
use futures::{future::join_all, join};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{ErrorType, NgsiLdMediaType, Uri},
    query::q,
    snapshot::{QueryDocument, SettledStatus, Snapshot, SnapshotClone, SnapshotFragment, SnapshotId, SnapshotStatus},
};

/// The Clone Snapshot requests sent at once under one clone id.
const PARALLEL_CLONES: usize = 8;

/// Pins: 8 Clone Snapshot requests of one source under one clone id sent at once answer one 201 and seven 409
/// `AlreadyExists`; an Update Snapshot Status, a Delete Snapshot and a Purge Snapshots selecting the clone sent at
/// once then answer 200 or 404 `ResourceNotFound`, 204 or 404 `ResourceNotFound`, and 204, 207 or 404
/// `ResourceNotFound`, and the clone is gone while its source remains
/// Clause: 5.16.2.4, 6.38.3.1, 5.16.4.4, 6.37.3.2, 5.16.5.4, 6.37.3.3, 5.16.7.4, 6.36.3.2
#[gtest]
#[tokio::test]
async fn n_snp_cln_8108_concurrent_clones_under_one_id_create_one_snapshot_and_a_racing_update_delete_and_purge_leave_it_gone() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let source_id = format!("urn:ngsi-ld:Snapshot:{namespace}-1").parse::<SnapshotId>()?;
    let clone_id = format!("urn:ngsi-ld:Snapshot:{namespace}-2").parse::<SnapshotId>()?;
    let source = Snapshot::builder()
        // Clone: the builder owns the id the test keeps addressing.
        .id(source_id.clone())
        .query(QueryDocument::builder().entities([vehicle_type.as_str()]).build()?)
        .build()?;
    broker.snapshots().create(&source).send().await?;
    await_status(&broker, &source_id, SnapshotStatus::Settled(SettledStatus::Empty)).await?;
    // Clone: the builder owns the id the test keeps addressing.
    let clone = SnapshotClone::builder().id(clone_id.clone()).build()?;
    let responding = broker.with_responses();
    let cloned = join_all((0..PARALLEL_CLONES).map(|_| async { responding.snapshots().clone_snapshot(&source_id, &clone).send().await })).await;
    let (created, refused): (Vec<_>, Vec<_>) = cloned.into_iter().partition(|answer| answer.answered_status() == Some(StatusCode::CREATED));
    expect_that!(created, len(eq(1)));
    expect_that!(refused, len(eq(PARALLEL_CLONES - 1)));
    expect_that!(refused, each(problem(StatusCode::CONFLICT, ErrorType::AlreadyExists)));
    await_status(&broker, &clone_id, SnapshotStatus::Settled(SettledStatus::Success)).await?;
    let fragment = SnapshotFragment::builder().priority(7).build()?;
    let selection = q::attr("id").eq(q::UnquotedUri::try_from(clone_id.to_string().parse::<Uri>()?)?);
    let racing = responding.snapshots();
    let (updated, deleted, purged) = join!(
        racing.update(&clone_id, &fragment).send(),
        racing.delete(&clone_id).send(),
        racing.purge(selection).send(),
    );
    expect_that!(
        &updated,
        any![answered(StatusCode::OK), problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound)]
    );
    expect_that!(
        &deleted,
        any![answered(StatusCode::NO_CONTENT), problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound)]
    );
    expect_that!(
        &purged,
        any![
            answered(StatusCode::NO_CONTENT),
            answered(StatusCode::MULTI_STATUS),
            problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound)
        ]
    );
    let gone = responding
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .snapshots()
        .retrieve(&clone_id)
        .send()
        .await;
    expect_that!(&gone, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    await_status(&broker, &source_id, SnapshotStatus::Settled(SettledStatus::Empty)).await?;
    broker.snapshots().delete(&source_id).send().await?;
    Ok(())
}
