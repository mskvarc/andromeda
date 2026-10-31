//! A retrieve whose single exclusive source does not answer in time adds warning 199 and no error status.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    cleanup::Cleanup,
    context_source::ContextSource,
    entity_matcher::has_attribute,
    namespace::Namespace,
    outcome::Done,
    problem::answered,
    source_answer,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::{
        UncheckedDocument,
        headers::{Accept, NgsiLdWarning, WarningCode},
        operation::RegisterContextSource,
    },
    model::{Entity, RawJson},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};
use std::time::Duration;

/// Pins: Retrieve Entity of a locally held Entity whose exclusive source answers after the registration timeout of
/// 500 ms answers 200 with the local Entity and NGSILD-Warning code 199
/// Clause: 6.3.17, 5.7.1.4
#[gtest]
#[tokio::test]
async fn n_fed_err_7207_a_retrieve_whose_single_exclusive_source_does_not_answer_in_time_adds_warning_199_and_no_error_status() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("brandName", "x")
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let registration = CSourceRegistration::builder(source.endpoint("a")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id(vehicle_id.clone())])
            .property_names(["speed"])
            .build()?])
        .mode(RegistrationMode::Exclusive)
        .operations([DistributedOperation::RetrieveEntity])
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
    let source_entity = Entity::output_builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 5)
        .build()?;
    source
        .answer(
            Method::GET,
            &format!("/a/ngsi-ld/v1/entities/{vehicle_id}"),
            source_answer::entity(&source_entity).set_delay(Duration::from_millis(3000)),
        )
        .await?;
    let retrieved = broker.with_accept(Accept::any()).entities().retrieve(&vehicle_id).send().await?;
    expect_that!(
        retrieved
            .warnings()
            .valid()
            .and_then(|warnings| warnings.iter().next())
            .map(NgsiLdWarning::code),
        some(eq(WarningCode::MiscellaneousWarning))
    );
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("brandName", any_attribute().value("x")));
    Ok(())
}
