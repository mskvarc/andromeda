//! System times carry at most six fraction digits and read back unchanged.

use andromeda_test_suite::{broker, namespace::Namespace};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::NgsiLdMediaType,
    snapshot::{QueryDocument, Snapshot, SnapshotId},
};

/// Pins: Create Snapshot stamps `createdAt` and `modifiedAt` with at most six fractional-second digits, and every
/// read of the Snapshot returns the same `createdAt`
/// Clause: 4.6.3, 4.8, 5.16.1.4, 5.16.3.4
#[gtest]
#[tokio::test]
async fn n_snp_cre_8155_system_times_carry_at_most_six_fraction_digits_and_read_back_unchanged() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let snapshot_id = format!("urn:ngsi-ld:Snapshot:{namespace}-1").parse::<SnapshotId>()?;
    let snapshot = Snapshot::builder()
        .id(snapshot_id.clone())
        .query(QueryDocument::builder().entities([vehicle_type.as_str()]).build()?)
        .build()?;
    broker.snapshots().create(&snapshot).send().await?;
    let pattern = r"^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}(\.[0-9]{1,6})?Z$";
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .snapshots()
        .retrieve(&snapshot_id)
        .send()
        .await?;
    let created = retrieved.output().created_at;
    expect_that!(created.as_ref().map(ToString::to_string), some(matches_regex(pattern)));
    expect_that!(retrieved.output().modified_at.as_ref().map(ToString::to_string), some(matches_regex(pattern)));
    let again = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .snapshots()
        .retrieve(&snapshot_id)
        .send()
        .await?;
    expect_that!(again.output().created_at, eq(created));
    expect_that!(again.output().modified_at.as_ref().map(ToString::to_string), some(matches_regex(pattern)));
    Ok(())
}
