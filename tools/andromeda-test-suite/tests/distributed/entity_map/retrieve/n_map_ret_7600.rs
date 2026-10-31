//! A map of local and peer entities reads back.

use andromeda_test_suite::{
    broker,
    config::{BrokerRole, PeerRole},
    namespace::Namespace,
    outcome::Done,
    problem::problem,
    received_status::ExpectedStatus,
    wire::request_to,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, ErrorType, NgsiLdMediaType},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, EntityMapId, EntitySource, RegistrationInfo, RegistrationMode},
};

/// Pins: Retrieve `EntityMap` on broker A of a map it created over a local Entity and one broker B serves answers
/// 200 with its id, type `EntityMap` and an `entityMap` mapping the local Entity to @none and the peer Entity to
/// its registration; a map id that is not a URI answers 400 `BadRequestData` and an unknown one 404
/// `ResourceNotFound`
/// Clause: 5.14.1.4, 5.2.39
#[gtest]
#[tokio::test]
async fn n_map_ret_7600_a_map_of_local_and_peer_entities_reads_back() -> Result<()> {
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
        .property("speed", 10)
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
        .operations([DistributedOperation::QueryEntity, DistributedOperation::CreateEntityMapQueryEntity])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    let created = broker
        .with_heads()
        .with_accept(Accept::any())
        .entity_maps()
        .create()
        .entity_type(&vehicle_type)
        .send()
        .await?
        .expecting(StatusCode::CREATED)?;
    let map_id = &created.body().id;
    let map = broker.with_accept(Accept::any()).entity_maps().retrieve(map_id).send().await?;
    expect_that!(&map.id, eq(map_id));
    expect_that!(&map.entity_map, len(eq(2)));
    expect_that!(
        map.entity_map.get(&vehicle_id).and_then(|sources| sources.first()),
        some(eq(&EntitySource::Local))
    );
    expect_that!(
        map.entity_map.get(&vehicle_2_id).and_then(|sources| sources.first()),
        some(eq(&EntitySource::Registration(registration_id)))
    );
    // A map id that is no URI is refused by the model before sending, so the request is sent as written.
    let request = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        "entityMaps/not-a-valid-uri",
    )?;
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let unknown_id = format!("urn:ngsi-ld:EntityMap:{namespace}-unknown").parse::<EntityMapId>()?;
    let unknown = broker
        .with_responses()
        .with_accept(Accept::any())
        .entity_maps()
        .retrieve(&unknown_id)
        .send()
        .await;
    expect_that!(&unknown, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    Ok(())
}
