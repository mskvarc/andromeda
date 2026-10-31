//! Contained by keeps a local entity linked from a peer entity out of a flat join.

use andromeda_test_suite::{
    broker,
    cleanup::Cleanup,
    config::{BrokerRole, PeerRole},
    identity_matcher::has_id,
    namespace::Namespace,
    nth::nth,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::Entity,
    query::Join,
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Query Entities on broker A by id and type with join flat and `joinLevel` 1 for an Entity broker B serves
/// through an inclusive registration, linking an Entity broker A stores, answers both, and with `containedBy`
/// naming the linked Entity answers the peer Entity only
/// Clause: 4.5.23.1, 4.5.23, 5.7.2.4, 6.4.3.2
#[gtest]
#[tokio::test]
async fn n_lnk_qry_7600_contained_by_keeps_a_local_entity_linked_from_a_peer_entity_out_of_a_flat_join() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_9_id = namespace.urn("Vehicle", 9)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let broker_b = broker::client(BrokerRole::B)?;
    let vehicle_9 = Entity::builder()
        .id(vehicle_9_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 5)
        .build()?;
    broker.entities().create(&vehicle_9).send().await?.done()?;
    let registration = CSourceRegistration::builder(broker::peer(PeerRole::B)?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern_ending("Vehicle", "1")?)])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::RetrieveEntity, DistributedOperation::QueryEntity])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .relationship("linkedTo", vehicle_9_id.clone())
        .build()?;
    broker_b.entities().create(&vehicle).send().await?.done()?;
    let page = broker
        .with_accept(Accept::any())
        .entities()
        .query()
        .ids(vec![vehicle_id.clone()])
        .entity_type(&vehicle_type)
        .join(Join::Flat)
        .join_level(1.try_into()?)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(2)));
    expect_that!(page.items(), contains(has_id(&vehicle_9_id)));
    let page_2 = broker
        .with_accept(Accept::any())
        .entities()
        .query()
        .ids(vec![vehicle_id.clone()])
        .entity_type(&vehicle_type)
        .join(Join::Flat)
        .join_level(1.try_into()?)
        .contained_by(vec![vehicle_9_id.clone()])
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(1)));
    expect_that!(page_2.items(), nth(0, has_id(&vehicle_id)));
    Ok(())
}
