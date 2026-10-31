//! A snapshot reads back the members it was created with.

use andromeda_test_suite::{broker, eventually::retried, namespace::Namespace, notification_sink::NotificationSink};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::{Accept, Location},
    model::{Duration, KeyValuePair, NgsiLdMediaType},
    query::temporal_query,
    snapshot::{QueryDocument, Snapshot, SnapshotId, SnapshotPriority, SnapshotStatus},
};

/// Pins: Create Snapshot with both kinds of query, `snapshotLifetime`, `snapshotPriority`, endpoint and
/// `receiverInfo` answers 201 with an empty body and its Location, and Retrieve Snapshot Status answers every
/// member as it was sent
/// Clause: 5.16.1.4, 5.16.3.4, Table 5.2.41-1, 6.36.3.1
#[gtest]
#[tokio::test]
async fn n_snp_cre_8116_a_snapshot_reads_back_the_members_it_was_created_with() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let snapshot_id = format!("urn:ngsi-ld:Snapshot:{namespace}-1").parse::<SnapshotId>()?;
    let status_endpoint = sink.endpoint("status")?;
    let snapshot = Snapshot::builder()
        .id(snapshot_id.clone())
        .query(QueryDocument::builder().entities([vehicle_type.as_str()]).build()?)
        .temporal_query(
            QueryDocument::temporal_builder(temporal_query::between("2025-01-01T00:00:00Z".parse()?, "2025-01-02T00:00:00Z".parse()?).build()?)
                .entities([vehicle_type.as_str()])
                .build()?,
        )
        .lifetime("PT1H".parse::<Duration>()?)
        .priority(6)
        .endpoint(status_endpoint.as_str())
        .receiver_info([KeyValuePair::new("X-Receiver", format!("model-{namespace}"))])
        .build()?;
    let created = broker.with_responses().snapshots().create(&snapshot).send().await?;
    expect_that!(
        created
            .head
            .typed::<Location>()
            .valid()
            .map(|location| location.as_iri_ref().as_str().to_owned()),
        some(eq(&format!("/ngsi-ld/v1/snapshots/{snapshot_id}")))
    );
    retried(async || {
        let retrieved = broker
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .snapshots()
            .retrieve(&snapshot_id)
            .send()
            .await?;
        verify_that!(retrieved.status(), eq("empty".parse::<SnapshotStatus>()?))?;
        verify_that!(retrieved.id(), eq(&snapshot_id))?;
        verify_that!(retrieved.queries().map(<[_]>::len), some(eq(1)))?;
        verify_that!(retrieved.temporal_queries().map(<[_]>::len), some(eq(1)))?;
        verify_that!(retrieved.lifetime().map(ToString::to_string), some(eq("PT1H")))?;
        verify_that!(retrieved.priority(), some(eq(SnapshotPriority::try_new(6)?)))?;
        verify_that!(retrieved.endpoint(), some(eq(&status_endpoint)))?;
        verify_that!(retrieved.receiver_info(), len(eq(1)))?;
        verify_that!(retrieved.receiver_info().first().map(KeyValuePair::key), some(eq("X-Receiver")))?;
        verify_that!(
            retrieved.receiver_info().first().map(KeyValuePair::value),
            some(eq(format!("model-{namespace}").as_str()))
        )?;
        Ok(())
    })
    .await?;
    broker.snapshots().delete(&snapshot_id).send().await?;
    Ok(())
}
