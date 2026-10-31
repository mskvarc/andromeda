//! A sub attribute equality under a clone matches the cloned instance.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, outcome::Done, snapshot_capture::await_status};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, Input, NgsiLdMediaType, Number, Property, Value},
    query::q,
    snapshot::{QueryDocument, SettledStatus, Snapshot, SnapshotClone, SnapshotId, SnapshotStatus},
};

/// Pins: under a clone of a Snapshot, a q equality on a sub-attribute matches the cloned Entity whose sub-attribute
/// holds the operand and no other
/// Clause: 5.5.15, 5.16.2.4, 4.9, 4.5.2
#[gtest]
#[tokio::test]
async fn n_snp_cln_0002_a_sub_attribute_equality_under_a_clone_matches_the_cloned_instance() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let device_type = namespace.entity_type("Device")?;
    let source_id = format!("urn:ngsi-ld:Snapshot:{namespace}-source").parse::<SnapshotId>()?;
    let clone_id = format!("urn:ngsi-ld:Snapshot:{namespace}-clone").parse::<SnapshotId>()?;
    let devices = [("1", "0.9", "21.5"), ("2", "0.5", "22.5")]
        .into_iter()
        .map(|(n, accuracy, temperature)| -> Result<Entity<Input>> {
            Ok(Entity::builder()
                .id(namespace.urn("Device", n)?)
                .entity_type(device_type.clone())
                .property(
                    "temperature",
                    Property::builder(Value::Number(temperature.parse()?)).property("accuracy", Value::Number(accuracy.parse()?)),
                )
                .build()?)
        })
        .collect::<Result<Vec<_>>>()?;
    broker.entities().batch_create(&devices).send().await?.done()?;
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
    for (operand, expected) in [("0.9", Some("1")), ("0.5", Some("2")), ("0.7", None)] {
        let page = broker
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .on_snapshot(&clone_id)
            .entities()
            .query()
            .entity_type(&device_type)
            .q(q::attr("temperature").sub_attr("accuracy").eq(operand.parse::<Number>()?))
            .send()
            .await?;
        match expected {
            Some(n) => {
                expect_that!(page.items(), len(eq(1)));
                expect_that!(page.items(), contains(has_id(&namespace.urn("Device", n)?)));
            }
            None => expect_that!(page.items(), is_empty()),
        }
    }
    broker.snapshots().delete(&clone_id).send().await?;
    broker.snapshots().delete(&source_id).send().await?;
    Ok(())
}
