//! A split capture decides a scope query on the merged entity before a pick leaving out scope reduces it.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    cleanup::Cleanup,
    config::BrokerRole,
    entity_matcher::{has_attribute, lacks_attribute},
    identity_matcher::has_id,
    namespace::Namespace,
    outcome::Done,
    peer_role::PeerRole,
    snapshot_capture::await_status,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::Entity,
    query::{EntitySplitting, projection::Projection, scope_query::ScopeQuery},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
    snapshot::{QueryDocument, SettledStatus, Snapshot, SnapshotId, SnapshotStatus},
};

/// Pins: A Snapshot whose query by type has `splitEntities` true, pick id,speed and `scopeQ` /Madrid, over an inclusive
/// registration of peer broker B, captures the Entity whose local part holds only brand and the scope /Madrid and
/// whose speed 70 B holds, reduced to speed with no scope, and not the Entity whose local part is scoped /Berlin and
/// whose speed 60 B holds
/// Clause: 5.16.1.4, 5.7.2.4, 5.7.2.5, 4.21, 4.3.6, 4.19
#[gtest]
#[tokio::test]
async fn n_snp_cre_8193_a_split_capture_decides_a_scope_query_on_the_merged_entity_before_a_pick_reduces_it() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let broker_b = broker::client(BrokerRole::B)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_2_id = namespace.urn("Vehicle", 2)?;
    let registration_id = namespace.registration_id(1)?;
    let snapshot_id = format!("urn:ngsi-ld:Snapshot:{namespace}-1").parse::<SnapshotId>()?;
    for (id, scope, speed) in [(&vehicle_id, "/Madrid", 70), (&vehicle_2_id, "/Berlin", 60)] {
        let part = Entity::builder()
            .id(id.clone())
            .entity_type(vehicle_type.clone())
            .scope(scope)
            .property("brand", "Mercedes")
            .build()?;
        broker.entities().create(&part).send().await?.done()?;
        let held = Entity::builder()
            .id(id.clone())
            .entity_type(vehicle_type.clone())
            .property("speed", speed)
            .build()?;
        broker_b.entities().create(&held).send().await?.done()?;
    }
    let registration = CSourceRegistration::builder(broker::peer(PeerRole::B)?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::QueryEntity])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    let snapshot = Snapshot::builder()
        .id(snapshot_id.clone())
        .query(
            QueryDocument::builder()
                .entities([vehicle_type.as_str()])
                .pick("id,speed".parse::<Projection>()?)
                .scope_q("/Madrid".parse::<ScopeQuery>()?)
                .split_entities(EntitySplitting::Split)
                .build()?,
        )
        .build()?;
    broker.snapshots().create(&snapshot).send().await?;
    await_status(&broker, &snapshot_id, SnapshotStatus::Settled(SettledStatus::Success)).await?;
    let page = broker
        .with_accept(Accept::any())
        .on_snapshot(&snapshot_id)
        .entities()
        .query()
        .entity_type(&vehicle_type)
        .send()
        .await?;
    expect_that!(
        page.items(),
        unordered_elements_are![all!(
            has_id(&vehicle_id),
            has_attribute("speed", any_attribute().value(70)),
            lacks_attribute("brand")
        )]
    );
    let item = page.items().first().or_fail()?;
    expect_that!(item.scope.as_ref(), none());
    broker.snapshots().delete(&snapshot_id).send().await?;
    Ok(())
}
