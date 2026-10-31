//! Snapshot status is served as json with a link or as json ld opening with its context.

use andromeda_test_suite::{
    broker,
    header_matcher::{absent, present},
    namespace::Namespace,
    snapshot_capture::await_status,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::{Accept, ContentType, Link},
    model::NgsiLdMediaType,
    snapshot::{QueryDocument, SettledStatus, Snapshot, SnapshotId, SnapshotStatus},
};

/// Pins: Retrieve Snapshot Status answers application/json with a Link header and no @context member, and
/// application/ld+json without a Link header and with the @context as its first member, `lastUsedAt` never moving
/// back
/// Clause: 5.16.3.4, 6.3.6, 6.37.3.1
#[gtest]
#[tokio::test]
async fn n_snp_sta_8100_snapshot_status_is_served_as_json_with_a_link_or_as_json_ld_opening_with_its_context() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let snapshot_id = format!("urn:ngsi-ld:Snapshot:{namespace}-1").parse::<SnapshotId>()?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let snapshot = Snapshot::builder()
        .id(snapshot_id.clone())
        .query(QueryDocument::builder().entities([vehicle_type.as_str()]).build()?)
        .build()?;
    broker.snapshots().create(&snapshot).send().await?;
    await_status(&broker, &snapshot_id, SnapshotStatus::Settled(SettledStatus::Empty)).await?;
    let as_json = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .snapshots()
        .retrieve(&snapshot_id)
        .send()
        .await?;
    expect_that!(
        as_json.head.typed::<ContentType>().valid(),
        some(eq(&ContentType::NgsiLd(NgsiLdMediaType::Json)))
    );
    expect_that!(as_json.head.typed::<Link>(), present());
    expect_that!(as_json.outcome.context(), none());
    let used = as_json.outcome.output().last_used_at.or_fail()?;
    let as_json_ld = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .snapshots()
        .retrieve(&snapshot_id)
        .send()
        .await?;
    expect_that!(
        as_json_ld.head.typed::<ContentType>().valid(),
        some(eq(&ContentType::NgsiLd(NgsiLdMediaType::JsonLd)))
    );
    expect_that!(as_json_ld.head.typed::<Link>(), absent());
    expect_that!(as_json_ld.outcome.context(), some(anything()));
    // The model keeps no member order, so the position of the @context is read from the body.
    expect_that!(String::from_utf8_lossy(&as_json_ld.body).into_owned(), contains_regex(r#"^\{\s*"@context""#));
    let used_again = as_json_ld.outcome.output().last_used_at.or_fail()?;
    expect_that!(used_again.timestamp(), ge(used.timestamp()));
    broker.snapshots().delete(&snapshot_id).send().await?;
    Ok(())
}
