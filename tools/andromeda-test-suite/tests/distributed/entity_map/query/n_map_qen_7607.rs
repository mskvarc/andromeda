//! A limit above the candidates maps every local and peer entity.

use andromeda_test_suite::{
    broker,
    config::{BrokerRole, PeerRole},
    namespace::Namespace,
    outcome::Done,
    wire::request_to,
};
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::ACCEPT};
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, EntityMap, RegistrationInfo, RegistrationMode},
};

/// Pins: Create `EntityMap` on broker A by type with limit 5, over a local Entity and one broker B serves through
/// an inclusive registration, answers 201 with both Entities mapped
/// Clause: 5.14.4.3, 5.14.4.4, 6.3.15
#[gtest]
#[tokio::test]
async fn n_map_qen_7607_a_limit_above_the_candidates_maps_every_local_and_peer_entity() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let broker_b = broker::client(BrokerRole::B)?;
    let vehicle_2_id = namespace.urn("Vehicle", 2)?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 1)
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let vehicle_2 = Entity::builder()
        .id(vehicle_2_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 2)
        .build()?;
    broker_b.entities().create(&vehicle_2).send().await?.done()?;
    let registration = CSourceRegistration::builder(broker::peer(PeerRole::B)?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern_ending("Vehicle", "2")?)])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::QueryEntity, DistributedOperation::CreateEntityMapQueryEntity])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // The creation takes no limit, so the request is sent as written.
    let request = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        &format!("entityMaps?type=Vehicle_{namespace}&limit=5"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let response = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request).await?;
    expect_that!(response.status(), eq(StatusCode::CREATED));
    let map = EntityMap::parse(str::from_utf8(response.body())?)?;
    expect_that!(&map.entity_map, len(eq(2)));
    expect_that!(map.entity_map.contains_key(&vehicle_id), eq(true));
    expect_that!(map.entity_map.contains_key(&vehicle_2_id), eq(true));
    Ok(())
}
