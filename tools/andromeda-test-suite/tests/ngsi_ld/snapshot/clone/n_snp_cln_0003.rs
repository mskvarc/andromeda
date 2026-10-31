//! A write to a clone leaves its source and the live entity unchanged.

use andromeda_test_suite::{
    attribute_matcher::property,
    broker,
    entity_matcher::{has_attribute, lacks_attribute},
    namespace::Namespace,
    outcome::Done,
    snapshot_capture::await_status,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, EntityFragment, NgsiLdMediaType, Property, Value},
    snapshot::{QueryDocument, SettledStatus, Snapshot, SnapshotClone, SnapshotId, SnapshotStatus},
};

/// Pins: a Merge Entity and an Append Entity Attributes under a clone of a Snapshot change the clone alone: the
/// merged value and the appended Attribute read under the clone, the merge keeps the sub-attribute the fragment
/// does not name, and the Snapshot it was cloned from and the live Entity read as before
/// Clause: 5.5.15, 5.16.2.4, 5.6.17.4, 5.6.3.4, 5.5.12
#[gtest]
#[tokio::test]
async fn n_snp_cln_0003_a_write_to_a_clone_leaves_its_source_and_the_live_entity_unchanged() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let device_type = namespace.entity_type("Device")?;
    let source_id = format!("urn:ngsi-ld:Snapshot:{namespace}-source").parse::<SnapshotId>()?;
    let clone_id = format!("urn:ngsi-ld:Snapshot:{namespace}-clone").parse::<SnapshotId>()?;
    let device_id = namespace.urn("Device", 1)?;
    let device = Entity::builder()
        .id(device_id.clone())
        .entity_type(device_type.clone())
        .property(
            "temperature",
            Property::builder(Value::Number("21.5".parse()?)).property("accuracy", Value::Number("0.9".parse()?)),
        )
        .build()?;
    broker.entities().create(&device).send().await?.done()?;
    let snapshot = Snapshot::builder()
        .id(source_id.clone())
        .query(QueryDocument::builder().entities([device_type.as_str()]).build()?)
        .build()?;
    broker.snapshots().create(&snapshot).send().await?;
    await_status(&broker, &source_id, SnapshotStatus::Settled(SettledStatus::Success)).await?;
    broker
        .snapshots()
        .clone_snapshot(&source_id, &SnapshotClone::builder().id(clone_id.clone()).build()?)
        .send()
        .await?;
    await_status(&broker, &clone_id, SnapshotStatus::Settled(SettledStatus::Success)).await?;
    let merge = EntityFragment::builder().property("temperature", Property::builder(30)).build()?;
    broker.on_snapshot(&clone_id).entities().merge(&device_id, &merge).send().await?.done()?;
    let append = EntityFragment::builder().property("humidity", Property::builder(55)).build()?;
    broker
        .on_snapshot(&clone_id)
        .entities()
        .append_attributes(&device_id, &append)
        .send()
        .await?
        .done()?;
    let under_clone = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .on_snapshot(&clone_id)
        .entities()
        .retrieve(&device_id)
        .send()
        .await?;
    expect_that!(
        under_clone.body(),
        has_attribute(
            "temperature",
            property().value(30).sub_attribute("accuracy", property().value(Value::Number("0.9".parse()?)))
        )
    );
    expect_that!(under_clone.body(), has_attribute("humidity", property().value(55)));
    let under_source = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .on_snapshot(&source_id)
        .entities()
        .retrieve(&device_id)
        .send()
        .await?;
    expect_that!(
        under_source.body(),
        has_attribute(
            "temperature",
            property()
                .value(Value::Number("21.5".parse()?))
                .sub_attribute("accuracy", property().value(Value::Number("0.9".parse()?)))
        )
    );
    expect_that!(under_source.body(), lacks_attribute("humidity"));
    let live = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&device_id)
        .send()
        .await?;
    expect_that!(
        live.body(),
        has_attribute(
            "temperature",
            property()
                .value(Value::Number("21.5".parse()?))
                .sub_attribute("accuracy", property().value(Value::Number("0.9".parse()?)))
        )
    );
    expect_that!(live.body(), lacks_attribute("humidity"));
    broker.snapshots().delete(&clone_id).send().await?;
    broker.snapshots().delete(&source_id).send().await?;
    Ok(())
}
