//! A clone system times carry at most six fraction digits and read back unchanged.

use andromeda_test_suite::{broker, namespace::Namespace};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::NgsiLdMediaType,
    snapshot::{QueryDocument, Snapshot, SnapshotClone, SnapshotId},
};

/// Pins: Clone Snapshot stamps the clone's `createdAt` and `modifiedAt` with at most six fractional-second digits,
/// and every read of the clone returns the same `createdAt`
/// Clause: 4.6.3, 4.8, 5.16.2.4, 5.16.3.4
#[gtest]
#[tokio::test]
async fn n_snp_cln_8107_a_clone_system_times_carry_at_most_six_fraction_digits_and_read_back_unchanged() -> Result<()> {
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
    broker
        .snapshots()
        .clone_snapshot(&source_id, &SnapshotClone::builder().id(clone_id.clone()).build()?)
        .send()
        .await?;
    let pattern = r"^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}(\.[0-9]{1,6})?Z$";
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .snapshots()
        .retrieve(&clone_id)
        .send()
        .await?;
    let created = retrieved.output().created_at;
    expect_that!(created.as_ref().map(ToString::to_string), some(matches_regex(pattern)));
    expect_that!(retrieved.output().modified_at.as_ref().map(ToString::to_string), some(matches_regex(pattern)));
    let again = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .snapshots()
        .retrieve(&clone_id)
        .send()
        .await?;
    expect_that!(again.output().created_at, eq(created));
    expect_that!(again.output().modified_at.as_ref().map(ToString::to_string), some(matches_regex(pattern)));
    Ok(())
}
