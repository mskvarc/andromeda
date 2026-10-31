//! Type and attribute discovery under a snapshot read the snapshot.

use andromeda_test_suite::{broker, eventually::retried, namespace::Namespace, outcome::Done, problem::answered, type_information::attribute_names};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType},
    snapshot::{QueryDocument, Snapshot, SnapshotId, SnapshotStatus},
};

/// Pins: Retrieve Available Entity Types, Entity Type Information, Available Attributes and Attribute Information
/// with NGSILD-Snapshot answer the type and Attribute of an Entity created under the Snapshot, which the live
/// discovery does not know
/// Clause: 5.5.15, 5.7.5.4, 5.7.7.4, 5.7.8.4, 5.7.10.4
#[gtest]
#[tokio::test]
async fn n_snp_qry_8109_type_and_attribute_discovery_under_a_snapshot_read_the_snapshot() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let snapshot_id = format!("urn:ngsi-ld:Snapshot:{namespace}-1").parse::<SnapshotId>()?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let velocity = namespace.attribute("velocity")?;
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
        .id(vehicle_id)
        .entity_type(vehicle_type.clone())
        .property(velocity.clone(), 4)
        .build()?;
    broker.on_snapshot(&snapshot_id).entities().create(&vehicle).send().await?.done()?;
    let json = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    let types = json.on_snapshot(&snapshot_id).entity_types().list().send().await?;
    expect_that!(types.type_list, contains(eq(&vehicle_type)));
    let type_information = json.on_snapshot(&snapshot_id).entity_types().retrieve(&vehicle_type).send().await?;
    expect_that!(type_information.entity_count, eq(1));
    expect_that!(attribute_names(&type_information).first(), some(eq(&velocity)));
    let attributes = json.on_snapshot(&snapshot_id).attributes().list().send().await?;
    expect_that!(attributes.attribute_list, contains(eq(&velocity)));
    let attribute_information = json.on_snapshot(&snapshot_id).attributes().retrieve(&velocity).send().await?;
    expect_that!(attribute_information.attribute_count, some(eq(1)));
    expect_that!(attribute_information.type_names.as_deref(), some(contains(eq(&vehicle_type))));
    let live_types = json.entity_types().list().send().await?;
    expect_that!(live_types.type_list, not(contains(eq(&vehicle_type))));
    let live_type_information = json.with_responses().entity_types().retrieve(&vehicle_type).send().await;
    expect_that!(&live_type_information, answered(StatusCode::NOT_FOUND));
    let live_attribute_information = json.with_responses().attributes().retrieve(&velocity).send().await;
    expect_that!(&live_attribute_information, answered(StatusCode::NOT_FOUND));
    broker.snapshots().delete(&snapshot_id).send().await?;
    Ok(())
}
