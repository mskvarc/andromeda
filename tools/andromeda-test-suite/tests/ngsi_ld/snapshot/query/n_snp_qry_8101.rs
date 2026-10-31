//! Attribute append update and delete under a snapshot change the snapshot alone.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::{has_attribute, lacks_attribute},
    eventually::retried,
    namespace::Namespace,
    outcome::Done,
    problem::answered,
    unchecked_call::exchange,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::headers::Accept,
    model::{AttributeName, Entity, EntityFragment, NgsiLdMediaType},
    snapshot::{QueryDocument, Snapshot, SnapshotId, SnapshotStatus},
};

/// Pins: Append Entity Attributes, Partial Attribute Update and Delete Attribute with NGSILD-Snapshot answer 204
/// and leave the Entity under the Snapshot with the updated value and without the deleted Attribute, while no
/// Entity exists live
/// Clause: 5.5.15, 5.6.3.4, 5.6.4.4, 5.6.5.4
#[gtest]
#[tokio::test]
async fn n_snp_qry_8101_attribute_append_update_and_delete_under_a_snapshot_change_the_snapshot_alone() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let snapshot_id = format!("urn:ngsi-ld:Snapshot:{namespace}-1").parse::<SnapshotId>()?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let snapshot = Snapshot::builder()
        .id(snapshot_id.clone())
        .query(QueryDocument::builder().entities([vehicle_type.as_str()]).build()?)
        .build()?;
    broker.snapshots().create(&snapshot).send().await?;
    retried(async || {
        let captured = broker
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .snapshots()
            .retrieve(&snapshot_id)
            .send()
            .await?;
        verify_that!(captured.status(), eq("empty".parse::<SnapshotStatus>()?))?;
        Ok(())
    })
    .await?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type)
        .property("speed", 10)
        .build()?;
    broker.on_snapshot(&snapshot_id).entities().create(&vehicle).send().await?.done()?;
    let fragment = EntityFragment::builder().property("color", "red").build()?;
    broker
        .on_snapshot(&snapshot_id)
        .entities()
        .append_attributes(&vehicle_id, &fragment)
        .send()
        .await?
        .done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .on_snapshot(&snapshot_id)
        .entities()
        .retrieve(&vehicle_id)
        .send()
        .await?;
    expect_that!(retrieved.body(), has_attribute("color", any_attribute().value("red")));
    // The concise writer renders a patch holding only a value as the bare value, so this one is sent as written.
    let patch = exchange(
        &broker,
        Method::PATCH,
        &format!("entities/{vehicle_id}/attrs/speed"),
        &[("Content-Type", "application/json"), ("NGSILD-Snapshot", snapshot_id.as_str())],
        Some(
            r#"{
    "value": 20
}"#,
        ),
    )?;
    let patched = broker.send_unchecked(patch).await;
    expect_that!(&patched, answered(StatusCode::NO_CONTENT));
    broker
        .on_snapshot(&snapshot_id)
        .entities()
        .delete_attribute(&vehicle_id, &"color".parse::<AttributeName>()?)
        .send()
        .await?
        .done()?;
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .on_snapshot(&snapshot_id)
        .entities()
        .retrieve(&vehicle_id)
        .send()
        .await?;
    expect_that!(retrieved_2.body(), has_attribute("speed", any_attribute().value(20)));
    expect_that!(retrieved_2.body(), lacks_attribute("color"));
    let live = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_id)
        .send()
        .await;
    expect_that!(&live, answered(StatusCode::NOT_FOUND));
    broker.snapshots().delete(&snapshot_id).send().await?;
    Ok(())
}
