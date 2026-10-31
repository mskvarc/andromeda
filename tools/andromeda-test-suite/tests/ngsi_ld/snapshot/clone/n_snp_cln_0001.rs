//! A clone reads an entity with nested sub attributes and several objects as the live read.

use andromeda_test_suite::{
    attribute_matcher::{property, relationship},
    broker,
    entity_matcher::has_attribute,
    namespace::Namespace,
    outcome::Done,
    snapshot_capture::await_status,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType, OneOrMany, Property, Relationship, Value},
    snapshot::{QueryDocument, SettledStatus, Snapshot, SnapshotClone, SnapshotId, SnapshotStatus},
};

/// Pins: an Entity holding a sub-attribute nested two deep and a Relationship with three objects reads, under the
/// Snapshot that captured it and under a clone of that Snapshot, exactly as it reads live
/// Clause: 4.3.7, 5.5.15, 5.16.1.4, 5.16.2.4, 4.5.2, 4.5.3
#[gtest]
#[tokio::test]
async fn n_snp_cln_0001_a_clone_reads_an_entity_with_nested_sub_attributes_and_several_objects_as_the_live_read() -> Result<()> {
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
            Property::builder(Value::Number("21.5".parse()?)).property(
                "accuracy",
                Property::builder(Value::Number("0.9".parse()?)).property("method", Property::builder("calibrated")),
            ),
        )
        .relationship(
            "hasPart",
            Relationship::builder(vec![namespace.urn("Part", "a")?, namespace.urn("Part", "b")?, namespace.urn("Part", "c")?]),
        )
        .build()?;
    broker.entities().create(&device).send().await?.done()?;
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
            property().sub_attribute("accuracy", property().sub_attribute("method", property().value("calibrated")))
        )
    );
    expect_that!(
        live.body(),
        has_attribute(
            "hasPart",
            relationship().object(OneOrMany::try_from(vec![
                namespace.urn("Part", "a")?,
                namespace.urn("Part", "b")?,
                namespace.urn("Part", "c")?
            ])?)
        )
    );
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
    let under_source = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .on_snapshot(&source_id)
        .entities()
        .retrieve(&device_id)
        .send()
        .await?;
    expect_that!(under_source.body(), eq(live.body()));
    let under_clone = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .on_snapshot(&clone_id)
        .entities()
        .retrieve(&device_id)
        .send()
        .await?;
    expect_that!(under_clone.body(), eq(live.body()));
    expect_that!(
        under_clone.body(),
        has_attribute(
            "temperature",
            property().sub_attribute("accuracy", property().sub_attribute("method", property().value("calibrated")))
        )
    );
    expect_that!(
        under_clone.body(),
        has_attribute(
            "hasPart",
            relationship().object(OneOrMany::try_from(vec![
                namespace.urn("Part", "a")?,
                namespace.urn("Part", "b")?,
                namespace.urn("Part", "c")?
            ])?)
        )
    );
    broker.snapshots().delete(&clone_id).send().await?;
    broker.snapshots().delete(&source_id).send().await?;
    Ok(())
}
