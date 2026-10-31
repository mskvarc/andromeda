//! A create whose single exclusive source is the broker itself is loop detected.

use andromeda_test_suite::{broker, config::PeerRole, namespace::Namespace, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    model::{ContextSourceAlias, Entity, ErrorType},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Create Entity matching a single exclusive registration whose endpoint and `contextSourceAlias` are the
/// broker itself answers 508 `LoopDetected`
/// Clause: 6.3.17, 5.6.1.4
#[gtest]
#[tokio::test]
async fn n_fed_err_7200_a_create_whose_single_exclusive_source_is_the_broker_itself_is_loop_detected() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration_id = namespace.registration_id(1)?;
    let registration = CSourceRegistration::builder(broker::peer(PeerRole::A)?)
        .id(registration_id)
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id(vehicle_id.clone())])
            .property_names(["speed"])
            .build()?])
        .mode(RegistrationMode::Exclusive)
        .operations([DistributedOperation::CreateEntity])
        .context_source_alias("broker-a".parse::<ContextSourceAlias>()?)
        .build()?;
    broker.registrations().register(&registration).send().await?;
    let vehicle = Entity::builder().id(vehicle_id).entity_type(vehicle_type).property("speed", 5).build()?;
    let answer_2 = broker.with_responses().entities().create(&vehicle).send().await;
    // The specification names no error type for a loop (clause 6.3.17), so the type is read as the IRI the broker writes.
    expect_that!(
        &answer_2,
        problem(
            StatusCode::LOOP_DETECTED,
            "https://uri.etsi.org/ngsi-ld/errors/LoopDetected".parse::<ErrorType>()?
        )
    );
    Ok(())
}
