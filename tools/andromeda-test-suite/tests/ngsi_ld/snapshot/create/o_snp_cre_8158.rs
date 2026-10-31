//! A temporal capture holds the latest instance a core write recorded.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    eventually::retried,
    evolution_matcher::{has_evolution, instance_id_at},
    namespace::Namespace,
    nth::nth,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, EntityFragment, NgsiLdMediaType, Property},
    query::temporal_query::{self, TemporalQuery},
    snapshot::{QueryDocument, Snapshot, SnapshotId, SnapshotStatus},
};

/// Pins: of a speed a Core write set to 10 and then to 20, a Snapshot whose temporal query selects the Vehicle
/// captures both instances: Retrieve Temporal Evolution under the Snapshot answers 10 and 20 under the
/// `instanceIds` the live Temporal Evolution answers
/// Clause: 4.3.1, 5.16.1.4, 5.7.3, 5.7.4.4
/// Recorded: clause 4.3.1 leaves it to the broker whether a Core API write updates the Temporal Evolution ("should
/// consider updating"); Andromeda records Core writes by default
#[gtest]
#[tokio::test]
async fn o_snp_cre_8158_a_temporal_capture_holds_the_latest_instance_a_core_write_recorded() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_captured_id = namespace.urn("Vehicle", "captured")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let snapshot_captured_id = format!("urn:ngsi-ld:Snapshot:{namespace}-captured").parse::<SnapshotId>()?;
    let vehicle_captured = Entity::builder()
        .id(vehicle_captured_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(10).observed_at("2025-01-01T00:00:00Z".parse()?))
        .build()?;
    broker.entities().create(&vehicle_captured).send().await?.done()?;
    let fragment = EntityFragment::builder()
        .property("speed", Property::builder(20).observed_at("2025-01-02T00:00:00Z".parse()?))
        .build()?;
    broker.entities().update_attributes(&vehicle_captured_id, &fragment).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_captured_id)
        .temporal_q(temporal_query::after("2024-01-01T00:00:00Z".parse()?))
        .send()
        .await?;
    let evolution = retrieved.body();
    expect_that!(evolution, has_evolution("speed", len(eq(2))));
    let replaced = instance_id_at(evolution, &"speed".parse()?, 0)?;
    let latest = instance_id_at(evolution, &"speed".parse()?, 1)?;
    let snapshot = Snapshot::builder()
        .id(format!("urn:ngsi-ld:Snapshot:{namespace}-captured").parse::<SnapshotId>()?)
        .temporal_query(
            QueryDocument::temporal_builder(TemporalQuery::after("2024-01-01T00:00:00Z".parse()?))
                .entities([vehicle_type.as_str()])
                .build()?,
        )
        .build()?;
    broker.snapshots().create(&snapshot).send().await?;
    retried(async || {
        let snapshot_2 = broker
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .snapshots()
            .retrieve(&snapshot_captured_id)
            .send()
            .await?;
        verify_that!(snapshot_2.status(), eq("success".parse::<SnapshotStatus>()?))?;
        Ok(())
    })
    .await?;
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .on_snapshot(&snapshot_captured_id)
        .temporal()
        .retrieve(&vehicle_captured_id)
        .temporal_q(temporal_query::after("2024-01-01T00:00:00Z".parse()?))
        .send()
        .await?;
    let evolution_2 = retrieved_2.body();
    expect_that!(
        evolution_2,
        has_evolution(
            "speed",
            all!(
                len(eq(2)),
                nth(0, any_attribute().value(10).instance_id(&replaced)),
                nth(1, any_attribute().value(20).instance_id(&latest))
            )
        )
    );
    broker.snapshots().delete(&snapshot_captured_id).send().await?;
    Ok(())
}
