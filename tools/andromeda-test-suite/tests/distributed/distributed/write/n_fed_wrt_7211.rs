//! A create spanning two inclusive sources of which one fails is a 207 naming only the failing registration.

use andromeda_test_suite::{
    broker,
    cleanup::Cleanup,
    context_source::ContextSource,
    namespace::Namespace,
    nth::nth,
    partial_outcome::PartialSuccess,
    report_matcher::batch_error,
    source_answer,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    model::Entity,
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo},
};

/// Pins: Create Entity matching two inclusive registrations, one source answering 201 and the other 500, answers
/// 207 with one error naming the failing registration only, and both sources are contacted once
/// Clause: 5.6.1.4, 6.3.17, 6.4.3.1
#[gtest]
#[tokio::test]
async fn n_fed_wrt_7211_a_create_spanning_two_inclusive_sources_of_which_one_fails_is_a_207_naming_only_the_failing_registration() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration_2_id = namespace.registration_id(2)?;
    let registration = CSourceRegistration::builder(source.endpoint("ok")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id(vehicle_id.clone())])
            .property_names(["speed"])
            .build()?])
        .operations([DistributedOperation::CreateEntity])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    let registration_2 = CSourceRegistration::builder(source.endpoint("bad")?)
        .id(registration_2_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id(vehicle_id.clone())])
            .property_names(["speed"])
            .build()?])
        .operations([DistributedOperation::CreateEntity])
        .build()?;
    broker.registrations().register(&registration_2).send().await?;
    let _registration_2_cleanup = Cleanup::of(&broker).registration(&registration_2_id);
    source
        .answer(Method::POST, "/ok/ngsi-ld/v1/entities", source_answer::status(StatusCode::CREATED))
        .await?;
    source
        .answer(
            Method::POST,
            "/bad/ngsi-ld/v1/entities",
            source_answer::status(StatusCode::INTERNAL_SERVER_ERROR),
        )
        .await?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 7)
        .build()?;
    let report = broker.entities().create(&vehicle).send().await?.partial()?;
    expect_that!(&report.errors, len(eq(1)));
    expect_that!(&report.success, nth(0, eq(&vehicle_id)));
    expect_that!(&report.errors, nth(0, batch_error().registration(&registration_2_id)));
    let forwarded = source.received_on(Method::POST, "/ok/ngsi-ld/v1/entities").await?;
    expect_that!(forwarded, len(eq(1)));
    let forwarded_2 = source.received_on(Method::POST, "/bad/ngsi-ld/v1/entities").await?;
    expect_that!(forwarded_2, len(eq(1)));
    Ok(())
}
