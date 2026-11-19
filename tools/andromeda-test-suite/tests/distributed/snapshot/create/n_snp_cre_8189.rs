//! A split temporal capture decides a geoquery on the history assembled from its parts.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    cleanup::Cleanup,
    config::{BrokerRole, PeerRole},
    evolution_matcher::has_evolution,
    identity_matcher::has_id,
    namespace::Namespace,
    nth::nth,
    outcome::Upserted,
    snapshot_capture::await_status,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{
        DateTime,
        GeoProperty,
        Property,
        evolution::TemporalEntity,
        geometry::{Position, Shape},
    },
    query::{
        EntitySplitting,
        geoquery,
        temporal_query::{self, TemporalQuery},
    },
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
    snapshot::{QueryDocument, SettledStatus, Snapshot, SnapshotId, SnapshotStatus},
};

/// Pins: A Snapshot whose temporal query after 2020-01-01 by type has `splitEntities` true and a near geoquery around
/// Madrid, over an inclusive `queryTemporal` registration of peer broker B, captures the history of the Entity whose
/// local part holds only brand and whose location in Madrid B holds, merged, the history B alone holds in Madrid and
/// the local one in Madrid, and none located in Berlin
/// Clause: 5.16.1.4, 5.7.4.4, 4.10
#[gtest]
#[tokio::test]
async fn n_snp_cre_8189_a_split_temporal_capture_decides_a_geoquery_on_the_history_assembled_from_its_parts() -> Result<()> {
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
    let since = "2020-01-01T00:00:00Z".parse::<DateTime>()?;
    let observed = "2020-08-01T12:00:00Z".parse::<DateTime>()?;
    let madrid = Shape::Point(Position::try_new(-3.7038, 40.4168)?);
    let berlin = Shape::Point(Position::try_new(13.405, 52.52)?);
    for (id, brand) in [(&vehicle_id, "Mercedes"), (&vehicle_2_id, "Volvo")] {
        let part = TemporalEntity::builder()
            .id(id.clone())
            .entity_type(vehicle_type.clone())
            .property("brand", Property::builder(brand).observed_at(observed))
            .build()?;
        broker.temporal().create_or_update(&part).send().await?.created()?;
    }
    for (id, place) in [(&vehicle_5_id, &madrid), (&vehicle_6_id, &berlin)] {
        let local = TemporalEntity::builder()
            .id(id.clone())
            .entity_type(vehicle_type.clone())
            .geo_property("location", GeoProperty::builder(place.clone()).observed_at(observed))
            .build()?;
        broker.temporal().create_or_update(&local).send().await?.created()?;
    }
    for (id, place) in [
        (&vehicle_id, &madrid),
        (&vehicle_2_id, &berlin),
        (&vehicle_3_id, &madrid),
        (&vehicle_4_id, &berlin),
    ] {
        let held = TemporalEntity::builder()
            .id(id.clone())
            .entity_type(vehicle_type.clone())
            .geo_property("location", GeoProperty::builder(place.clone()).observed_at(observed))
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
                .geo_q(geoquery::near_max(3000, madrid).build()?)
                .split_entities(EntitySplitting::Split)
                .build()?,
        )
        .build()?;
    broker.snapshots().create(&snapshot).send().await?;
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
                has_evolution("location", len(eq(1))),
                has_evolution("brand", all!(len(eq(1)), nth(0, any_attribute().value("Mercedes"))))
            ),
            all!(has_id(&vehicle_3_id), has_evolution("location", len(eq(1)))),
            all!(has_id(&vehicle_5_id), has_evolution("location", len(eq(1))))
        ]
    );
    broker.snapshots().delete(&snapshot_id).send().await?;
    Ok(())
}
