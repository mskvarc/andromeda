//! A split temporal capture decides q on the history assembled from its parts.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    cleanup::Cleanup,
    config::BrokerRole,
    evolution_matcher::has_evolution,
    identity_matcher::has_id,
    namespace::Namespace,
    nth::nth,
    outcome::Upserted,
    peer_role::PeerRole,
    snapshot_capture::{SnapshotQuery, await_query_result, await_status},
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{DateTime, Property, evolution::TemporalEntity},
    query::{
        EntitySplitting,
        q,
        temporal_query::{self, TemporalQuery},
    },
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
    snapshot::{ExecutionStatus, QueryDocument, SettledStatus, Snapshot, SnapshotId, SnapshotStatus},
};

/// Pins: A Snapshot whose temporal query after 2020-01-01 by type has `splitEntities` true and q speed>50, over an
/// inclusive `queryTemporal` registration of peer broker B, captures the history of the Entity whose local part
/// holds only brand and whose speed of 70 B holds, merged, the history B alone holds with speed 80 and the local one
/// with speed 90, and none whose speed is 30, 20 or 10; a second Snapshot with q speed>100 captures none of them and
/// reports its temporal query empty
/// Clause: 5.16.1.4, 5.7.4.4, 5.2.42
#[gtest]
#[tokio::test]
async fn n_snp_cre_8188_a_split_temporal_capture_decides_q_on_the_history_assembled_from_its_parts() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let broker_b = broker::client(BrokerRole::B)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_2_id = namespace.urn("Vehicle", 2)?;
    let vehicle_3_id = namespace.urn("Vehicle", 3)?;
    let vehicle_4_id = namespace.urn("Vehicle", 4)?;
    let vehicle_5_id = namespace.urn("Vehicle", 5)?;
    let vehicle_6_id = namespace.urn("Vehicle", 6)?;
    let registration_id = namespace.registration_id(1)?;
    let snapshot_id = format!("urn:ngsi-ld:Snapshot:{namespace}-1").parse::<SnapshotId>()?;
    let snapshot_2_id = format!("urn:ngsi-ld:Snapshot:{namespace}-2").parse::<SnapshotId>()?;
    let since = "2020-01-01T00:00:00Z".parse::<DateTime>()?;
    let observed = "2020-08-01T12:00:00Z".parse::<DateTime>()?;
    for (id, brand) in [(&vehicle_id, "Mercedes"), (&vehicle_2_id, "Volvo")] {
        let part = TemporalEntity::builder()
            .id(id.clone())
            .entity_type(vehicle_type.clone())
            .property("brand", Property::builder(brand).observed_at(observed))
            .build()?;
        broker.temporal().create_or_update(&part).send().await?.created()?;
    }
    for (id, speed) in [(&vehicle_5_id, 90), (&vehicle_6_id, 10)] {
        let local = TemporalEntity::builder()
            .id(id.clone())
            .entity_type(vehicle_type.clone())
            .property("speed", Property::builder(speed).observed_at(observed))
            .build()?;
        broker.temporal().create_or_update(&local).send().await?.created()?;
    }
    for (id, speed) in [(&vehicle_id, 70), (&vehicle_2_id, 30), (&vehicle_3_id, 80), (&vehicle_4_id, 20)] {
        let held = TemporalEntity::builder()
            .id(id.clone())
            .entity_type(vehicle_type.clone())
            .property("speed", Property::builder(speed).observed_at(observed))
            .build()?;
        broker_b.temporal().create_or_update(&held).send().await?.created()?;
    }
    let registration = CSourceRegistration::builder(broker::peer(PeerRole::B)?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::QueryTemporal])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    let snapshot = Snapshot::builder()
        .id(snapshot_id.clone())
        .temporal_query(
            QueryDocument::temporal_builder(TemporalQuery::after(since))
                .entities([vehicle_type.as_str()])
                .q(q::attr("speed").gt(50).build()?)
                .split_entities(EntitySplitting::Split)
                .build()?,
        )
        .build()?;
    broker.snapshots().create(&snapshot).send().await?;
    let snapshot_2 = Snapshot::builder()
        .id(snapshot_2_id.clone())
        .temporal_query(
            QueryDocument::temporal_builder(TemporalQuery::after(since))
                .entities([vehicle_type.as_str()])
                .q(q::attr("speed").gt(100).build()?)
                .split_entities(EntitySplitting::Split)
                .build()?,
        )
        .build()?;
    broker.snapshots().create(&snapshot_2).send().await?;
    await_status(&broker, &snapshot_id, SnapshotStatus::Settled(SettledStatus::Success)).await?;
    let page = broker
        .with_accept(Accept::any())
        .on_snapshot(&snapshot_id)
        .temporal()
        .query(temporal_query::after(since))
        .entity_type(&vehicle_type)
        .send()
        .await?;
    expect_that!(
        page.items(),
        unordered_elements_are![
            all!(
                has_id(&vehicle_id),
                has_evolution("speed", all!(len(eq(1)), nth(0, any_attribute().value(70)))),
                has_evolution("brand", all!(len(eq(1)), nth(0, any_attribute().value("Mercedes"))))
            ),
            all!(
                has_id(&vehicle_3_id),
                has_evolution("speed", all!(len(eq(1)), nth(0, any_attribute().value(80))))
            ),
            all!(
                has_id(&vehicle_5_id),
                has_evolution("speed", all!(len(eq(1)), nth(0, any_attribute().value(90))))
            )
        ]
    );
    let detail = await_query_result(&broker, &snapshot_2_id, SettledStatus::Empty, SnapshotQuery::Temporal(0)).await?;
    expect_that!(detail.result_status, eq(ExecutionStatus::Empty));
    let page_2 = broker
        .with_accept(Accept::any())
        .on_snapshot(&snapshot_2_id)
        .temporal()
        .query(temporal_query::after(since))
        .entity_type(&vehicle_type)
        .send()
        .await?;
    expect_that!(page_2.items(), is_empty());
    broker.snapshots().delete(&snapshot_id).send().await?;
    broker.snapshots().delete(&snapshot_2_id).send().await?;
    Ok(())
}
