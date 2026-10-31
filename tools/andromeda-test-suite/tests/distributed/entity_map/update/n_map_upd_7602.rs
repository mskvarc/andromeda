//! An expiry that is not a datetime is bad request data and changes nothing.

use andromeda_test_suite::{
    broker,
    cleanup::Cleanup,
    config::{BrokerRole, PeerRole},
    namespace::Namespace,
    outcome::Done,
    problem::problem,
    received_status::ExpectedStatus,
    wire::request_to,
};
use bytes::Bytes;
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::CONTENT_TYPE};
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, ErrorType},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Update `EntityMap` on broker A with `expiresAt` not-a-datetime answers 400 `BadRequestData`, and the map
/// reads back unchanged
/// Clause: 5.14.2.4, 5.2.39
#[gtest]
#[tokio::test]
async fn n_map_upd_7602_an_expiry_that_is_not_a_datetime_is_bad_request_data_and_changes_nothing() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let broker_b = broker::client(BrokerRole::B)?;
    let vehicle_2_id = namespace.urn("Vehicle", 2)?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle = Entity::builder()
        .id(vehicle_id)
        .entity_type(vehicle_type.clone())
        .property("speed", 1)
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let vehicle_2 = Entity::builder()
        .id(vehicle_2_id)
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
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
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
    // The body is no expiry the model writes, so it is sent as written.
    let request = request_to(&broker, Method::PATCH, &format!("entityMaps/{map_id}"))?
        .append_header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
        .body(Bytes::from("{\n    \"expiresAt\": \"not-a-datetime\"\n}"));
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let map = broker.with_accept(Accept::any()).entity_maps().retrieve(map_id).send().await?;
    expect_that!(&map.expires_at, eq(&created.body().expires_at));
    expect_that!(&map.entity_map, len(eq(2)));
    Ok(())
}
