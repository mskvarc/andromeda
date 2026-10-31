//! An operation on a snapshot does not see later changes.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::has_attribute,
    eventually::retried,
    namespace::Namespace,
    nth::nth,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::{Accept, NgsiLdSnapshot},
    model::{Entity, EntityFragment, NgsiLdMediaType},
    snapshot::{QueryDocument, Snapshot, SnapshotId, SnapshotStatus},
};

/// Pins: Query Entities with NGSILD-Snapshot naming a Snapshot answers the Entity as it was captured, with the same
/// header, while the live Entity has since changed
/// Clause: 5.5.15, 5.16.1.4, 6.3.22
#[gtest]
#[tokio::test]
async fn n_snp_qry_6400_an_operation_on_a_snapshot_does_not_see_later_changes() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let snapshot_id = format!("urn:ngsi-ld:Snapshot:{namespace}-1").parse::<SnapshotId>()?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 10)
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let snapshot = Snapshot::builder()
        .id(format!("urn:ngsi-ld:Snapshot:{namespace}-1").parse::<SnapshotId>()?)
        .query(QueryDocument::builder().entities([vehicle_type.as_str()]).build()?)
        .lifetime("PT1H".parse()?)
        .build()?;
    broker.snapshots().create(&snapshot).send().await?;
    retried(async || {
        let snapshot_2 = broker
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .snapshots()
            .retrieve(&snapshot_id)
            .send()
            .await?;
        verify_that!(snapshot_2.status(), eq("success".parse::<SnapshotStatus>()?))?;
        Ok(())
    })
    .await?;
    let fragment = EntityFragment::builder().property("speed", 99).build()?;
    broker.entities().update_attributes(&vehicle_id, &fragment).send().await?.done()?;
    let headed = broker
        .with_heads()
        .with_accept(Accept::any())
        .on_snapshot(&snapshot_id)
        .entities()
        .query()
        .entity_type(&vehicle_type)
        .ids(vec![vehicle_id.clone()])
        .send()
        .await?;
    let page = headed.outcome;
    expect_that!(
        headed.head.typed::<NgsiLdSnapshot>().valid().map(NgsiLdSnapshot::snapshot),
        some(eq(&snapshot_id))
    );
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_attribute("speed", any_attribute().value(10))));
    let page_2 = broker
        .with_accept(Accept::any())
        .entities()
        .query()
        .entity_type(&vehicle_type)
        .ids(vec![vehicle_id.clone()])
        .send()
        .await?;
    expect_that!(page_2.items(), nth(0, has_attribute("speed", any_attribute().value(99))));
    broker.snapshots().delete(&snapshot_id).send().await?;
    Ok(())
}
