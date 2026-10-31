//! A full purge answers 204 without content and a purge of nothing 404.

use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    notification_sink::NotificationSink,
    problem::{answered, problem, problem_status},
};
use googletest::prelude::*;
use http::{
    StatusCode,
    header::{CONTENT_LENGTH, CONTENT_TYPE},
};
use ngsi_ld::{
    http::{headers::Accept, operation::PurgeSnapshots},
    model::{ErrorType, NgsiLdMediaType},
    query::q,
    snapshot::{QueryDocument, Snapshot, SnapshotId, SnapshotPriority},
};

/// Pins: Purge Snapshots selecting two Snapshots answers 204 without content and deletes both while the unselected
/// Snapshot remains; the same purge again answers 404 `ResourceNotFound` as application/json
/// Clause: 5.16.7.4, 6.36.3.2
#[gtest]
#[tokio::test]
async fn n_snp_prg_8103_a_full_purge_answers_204_without_content_and_a_purge_of_nothing_404() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let endpoint = sink.endpoint("purge")?;
    let snapshot_ids = ["1", "2", "3"].map(|suffix| format!("urn:ngsi-ld:Snapshot:{namespace}-{suffix}").parse::<SnapshotId>());
    let [snapshot_1_id, snapshot_2_id, snapshot_3_id] = snapshot_ids;
    let (snapshot_1_id, snapshot_2_id, snapshot_3_id) = (snapshot_1_id?, snapshot_2_id?, snapshot_3_id?);
    for (id, priority) in [(&snapshot_1_id, 4), (&snapshot_2_id, 4), (&snapshot_3_id, 8)] {
        let snapshot = Snapshot::builder()
            .id(id.clone())
            .query(QueryDocument::builder().entities([vehicle_type.as_str()]).build()?)
            .priority(priority)
            .endpoint(endpoint.clone())
            .build()?;
        broker.snapshots().create(&snapshot).send().await?;
    }
    let selection = q::attr("snapshotPriority").eq(4).and(q::attr("endpoint").eq(endpoint.as_str()));
    let query = selection.clone().build()?;
    // The typed purge keeps no body or header field of its answer, which the test states.
    let purged = broker.send_unchecked(broker.unchecked(&PurgeSnapshots { q: &query })?).await;
    expect_that!(&purged, answered(StatusCode::NO_CONTENT).without_header(CONTENT_LENGTH).without_body());
    for id in [&snapshot_1_id, &snapshot_2_id] {
        let answer = broker
            .with_responses()
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .snapshots()
            .retrieve(id)
            .send()
            .await;
        expect_that!(&answer, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    }
    let remaining = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .snapshots()
        .retrieve(&snapshot_3_id)
        .send()
        .await?;
    expect_that!(remaining.priority(), some(eq(SnapshotPriority::try_new(8)?)));
    let again = broker.with_responses().snapshots().purge(selection).send().await;
    expect_that!(
        &again,
        problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound).with_header(CONTENT_TYPE, "application/json")
    );
    expect_that!(problem_status(&again), some(eq(404)));
    broker.snapshots().delete(&snapshot_3_id).send().await?;
    Ok(())
}
