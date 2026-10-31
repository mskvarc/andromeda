//! A failed source is not contacted again during its cooldown and the retrieve adds warning 199.

use andromeda_test_suite::{broker, cleanup::Cleanup, context_source::ContextSource, namespace::Namespace, outcome::Done, problem::answered, source_answer};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::{
        UncheckedDocument,
        headers::{Accept, NgsiLdWarning, WarningCode},
        operation::RegisterContextSource,
    },
    model::{Entity, RawJson},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo},
};

/// Pins: Retrieve Entity twice with an inclusive source that answers 500 and a cooldown of 60000 ms answers warning
/// 299 the first time and warning 199 the second, and the source receives one request
/// Clause: 6.3.17, 5.2.34
/// Recorded: clause 6.3.17 lets a broker optionally decline further requests to a failed endpoint during its
/// cooldown, and Table 5.2.34-1 leaves the warning code of the declined request to the broker
#[gtest]
#[tokio::test]
async fn o_fed_err_7209_a_failed_source_is_not_contacted_again_during_its_cooldown_and_the_retrieve_adds_warning_199() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 5)
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let registration = CSourceRegistration::builder(source.endpoint("a")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id(vehicle_id.clone())])
            .property_names(["speed"])
            .build()?])
        .operations([DistributedOperation::RetrieveEntity])
        .build()?;
    // The model refuses it (a registration management), so `management` is written as text.
    let management: RawJson = r#"{"cooldown": 60000}"#.parse()?;
    let document = UncheckedDocument::try_from(registration.render())?.set_member("management", &management);
    let request = broker.unchecked(&RegisterContextSource { registration: &registration })?.document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    source
        .answer(
            Method::GET,
            &format!("/a/ngsi-ld/v1/entities/{vehicle_id}"),
            source_answer::status(StatusCode::INTERNAL_SERVER_ERROR),
        )
        .await?;
    let retrieved = broker.with_accept(Accept::any()).entities().retrieve(&vehicle_id).send().await?;
    expect_that!(
        retrieved
            .warnings()
            .valid()
            .and_then(|warnings| warnings.iter().next())
            .map(NgsiLdWarning::code),
        some(eq(WarningCode::MiscellaneousPersistentWarning))
    );
    let retrieved_2 = broker.with_accept(Accept::any()).entities().retrieve(&vehicle_id).send().await?;
    expect_that!(
        retrieved_2
            .warnings()
            .valid()
            .and_then(|warnings| warnings.iter().next())
            .map(NgsiLdWarning::code),
        some(eq(WarningCode::MiscellaneousWarning))
    );
    let forwarded = source.received_on(Method::GET, &format!("/a/ngsi-ld/v1/entities/{vehicle_id}")).await?;
    expect_that!(forwarded, len(eq(1)));
    Ok(())
}
