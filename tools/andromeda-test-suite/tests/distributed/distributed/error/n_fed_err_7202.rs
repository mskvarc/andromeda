//! A create whose single exclusive source does not answer in time is gateway timeout.

use andromeda_test_suite::{
    broker,
    cleanup::Cleanup,
    context_source::ContextSource,
    namespace::Namespace,
    problem::{answered, problem},
    source_answer,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::{UncheckedDocument, operation::RegisterContextSource},
    model::{Entity, ErrorType, RawJson},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};
use std::time::Duration;

/// Pins: Create Entity matching a single exclusive registration whose source answers after the registration timeout
/// of 500 ms answers 504 `GatewayTimeout`
/// Clause: 6.3.17, 5.2.34, 5.6.1.4
#[gtest]
#[tokio::test]
async fn n_fed_err_7202_a_create_whose_single_exclusive_source_does_not_answer_in_time_is_gateway_timeout() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration = CSourceRegistration::builder(source.endpoint("a")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id(vehicle_id.clone())])
            .property_names(["speed"])
            .build()?])
        .mode(RegistrationMode::Exclusive)
        .operations([DistributedOperation::CreateEntity])
        .build()?;
    // The model refuses it (a registration management), so `management` is written as text.
    let management: RawJson = r#"{"timeout": 500}"#.parse()?;
    let document = UncheckedDocument::try_from(registration.render())?.set_member("management", &management);
    let request = broker.unchecked(&RegisterContextSource { registration: &registration })?.document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    source
        .answer(
            Method::POST,
            "/a/ngsi-ld/v1/entities",
            source_answer::status(StatusCode::CREATED).set_delay(Duration::from_millis(3000)),
        )
        .await?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 5)
        .build()?;
    let answer_2 = broker.with_responses().entities().create(&vehicle).send().await;
    expect_that!(
        &answer_2,
        problem(
            StatusCode::GATEWAY_TIMEOUT,
            "https://uri.etsi.org/ngsi-ld/errors/GatewayTimeout".parse::<ErrorType>()?
        )
    );
    Ok(())
}
