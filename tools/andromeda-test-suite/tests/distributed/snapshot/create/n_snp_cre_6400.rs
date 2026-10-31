//! A snapshot created without an id answers an empty 201 with its location.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done, snapshot_capture::await_status};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::{Accept, Location},
    model::{Duration, Entity, NgsiLdMediaType},
    snapshot::{QueryDocument, Snapshot, SnapshotPriority, SnapshotStatus},
};

/// Pins: Create Snapshot without an id answers 201 with an empty body and a Location under /snapshots/ ending in
/// the generated identifier, and the Snapshot reads back with the default `snapshotPriority` 5 and status success
/// Clause: 5.16.1.4, 5.16.1.5, 6.4.3.1
#[gtest]
#[tokio::test]
async fn n_snp_cre_6400_a_snapshot_created_without_an_id_answers_an_empty_201_with_its_location() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle = Entity::builder()
        .id(namespace.urn("Vehicle", 1)?)
        .entity_type(vehicle_type.clone())
        .property("speed", 10)
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let snapshot = Snapshot::builder()
        .lifetime("PT1H".parse::<Duration>()?)
        .query(QueryDocument::builder().entities([vehicle_type.as_str()]).build()?)
        .build()?;
    let created = broker.with_responses().snapshots().create(&snapshot).send().await?;
    expect_that!(created.body.len(), eq(0));
    let location = created
        .head
        .typed::<Location>()
        .valid()
        .map(|location| location.as_iri_ref().as_str().to_owned());
    expect_that!(location, some(all!(contains_substring("/snapshots/"), contains_regex("[^/]$"))));
    let snapshot_id = created.outcome.id().or_fail()?;
    await_status(&broker, &snapshot_id, "success".parse::<SnapshotStatus>()?).await?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .snapshots()
        .retrieve(&snapshot_id)
        .send()
        .await?;
    expect_that!(retrieved.status(), eq("success".parse::<SnapshotStatus>()?));
    expect_that!(retrieved.priority(), some(eq(SnapshotPriority::try_new(5)?)));
    broker.snapshots().delete(&snapshot_id).send().await?;
    Ok(())
}
