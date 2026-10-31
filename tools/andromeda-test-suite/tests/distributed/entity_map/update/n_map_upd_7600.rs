//! An expiry reads back with the map intact.

use andromeda_test_suite::{
    broker,
    config::{BrokerRole, PeerRole},
    namespace::Namespace,
    outcome::Done,
    received_status::ExpectedStatus,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{headers::Accept, operation::EntityMapRequest},
    model::{DateTime, Entity},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, EntitySource, RegistrationInfo, RegistrationMode},
};

/// Pins: Update `EntityMap` on broker A with `expiresAt` 2099-01-01T00:00:00Z of a map it created over a local and a
/// peer Entity answers 204, and the map reads back with that `expiresAt` and both Entities still mapped
/// Clause: 5.14.2.4, 6.32.3.2
#[gtest]
#[tokio::test]
async fn n_map_upd_7600_an_expiry_reads_back_with_the_map_intact() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let broker_b = broker::client(BrokerRole::B)?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_2_id = namespace.urn("Vehicle", 2)?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 15)
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let vehicle_2 = Entity::builder()
        .id(vehicle_2_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 20)
        .build()?;
    broker_b.entities().create(&vehicle_2).send().await?.done()?;
    let registration = CSourceRegistration::builder(broker::peer(PeerRole::B)?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern_ending("Vehicle", "2")?)])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::QueryEntity])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    let entity_map_request = EntityMapRequest::returned();
    let page = broker
        .with_heads()
        .with_accept(Accept::any())
        .entities()
        .query()
        .entity_type(&vehicle_type)
        .entity_map(&entity_map_request)
        .send()
        .await?
        .expecting(StatusCode::CREATED)?;
    let map_id = page.entity_map().valid().or_fail()?.id().or_fail()?;
    let expires_at: DateTime = "2099-01-01T00:00:00Z".parse()?;
    broker.entity_maps().update(&map_id, &expires_at).send().await?;
    let map = broker.with_accept(Accept::any()).entity_maps().retrieve(&map_id).send().await?;
    expect_that!(&map.expires_at, eq(&expires_at));
    expect_that!(map.entity_map.get(&vehicle_id), some(anything()));
    expect_that!(
        map.entity_map.get(&vehicle_2_id),
        some(contains(eq(&EntitySource::Registration(registration_id))))
    );
    Ok(())
}
