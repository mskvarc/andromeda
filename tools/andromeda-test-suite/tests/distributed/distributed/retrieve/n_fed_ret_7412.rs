//! An entity neither held nor held by an auxiliary peer is not found.

use andromeda_test_suite::{broker, cleanup::Cleanup, config::PeerRole, namespace::Namespace, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::ErrorType,
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Retrieve Entity on broker A of an Entity neither A nor the auxiliary registration's broker B holds answers
/// 404 `ResourceNotFound`
/// Clause: 5.7.1.4
#[gtest]
#[tokio::test]
async fn n_fed_ret_7412_an_entity_neither_held_nor_held_by_an_auxiliary_peer_is_not_found() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration_id = namespace.registration_id(1)?;
    let registration = CSourceRegistration::builder(broker::peer(PeerRole::B)?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id(vehicle_id.clone())])
            .property_names(["speed", "fuelLevel"])
            .build()?])
        .mode(RegistrationMode::Auxiliary)
        .operations([DistributedOperation::RetrieveEntity])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    let answer = broker.with_responses().with_accept(Accept::any()).entities().retrieve(&vehicle_id).send().await;
    expect_that!(&answer, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    Ok(())
}
