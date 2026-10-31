//! A create into a redirect cycle is loop detected and stored by no broker.

use andromeda_test_suite::{
    broker,
    cleanup::Cleanup,
    config::{BrokerRole, PeerRole},
    namespace::Namespace,
    problem::{answered, problem},
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, ErrorType},
    query::Locality,
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Create Entity on broker A of a Vehicle whose type A redirects to broker B, which redirects it back to A,
/// answers 508 `LoopDetected`, and neither broker holds the Vehicle
/// Clause: 6.3.17, 4.3.6.3, 6.3.18, 5.6.1.4
#[gtest]
#[tokio::test]
async fn n_fed_err_6404_a_create_into_a_redirect_cycle_is_loop_detected_and_stored_by_no_broker() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let broker_b = broker::client(BrokerRole::B)?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration_id = namespace.registration_id(1)?;
    let registration_2_id = namespace.registration_id(2)?;
    let registration = CSourceRegistration::builder(broker::peer(PeerRole::B)?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .mode(RegistrationMode::Redirect)
        .operations([DistributedOperation::CreateEntity])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    let registration_2 = CSourceRegistration::builder(broker::peer(PeerRole::A)?)
        .id(registration_2_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .mode(RegistrationMode::Redirect)
        .operations([DistributedOperation::CreateEntity])
        .build()?;
    broker_b.registrations().register(&registration_2).send().await?;
    let _registration_2_cleanup = Cleanup::of(&broker_b).registration(&registration_2_id);
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type)
        .property("speed", 5)
        .build()?;
    let created = broker.with_responses().entities().create(&vehicle).send().await;
    // The specification names no error type for a loop (clause 6.3.17), so the type is read as the IRI the broker writes.
    expect_that!(
        &created,
        problem(
            StatusCode::LOOP_DETECTED,
            "https://uri.etsi.org/ngsi-ld/errors/LoopDetected".parse::<ErrorType>()?
        )
    );
    for holder in [&broker, &broker_b] {
        let held = holder
            .with_responses()
            .with_accept(Accept::any())
            .entities()
            .retrieve(&vehicle_id)
            .locality(Locality::Local)
            .send()
            .await;
        expect_that!(&held, answered(StatusCode::NOT_FOUND));
    }
    Ok(())
}
