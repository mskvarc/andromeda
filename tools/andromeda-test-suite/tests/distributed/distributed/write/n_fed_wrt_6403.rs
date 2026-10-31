//! A context source info content length cannot override the real one.

use andromeda_test_suite::{
    broker,
    cleanup::Cleanup,
    context_source::ContextSource,
    header_condition::HeaderCondition,
    namespace::Namespace,
    outcome::Done,
    problem::answered,
    source_answer,
    source_request::SourceRequest,
};
use googletest::prelude::*;
use http::{HeaderName, Method, StatusCode};
use ngsi_ld::{
    http::{UncheckedDocument, operation::RegisterContextSource},
    model::{Entity, RawJson},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Create Entity under an inclusive registration whose `contextSourceInfo` names Content-Length 9999 is
/// forwarded once, and no forwarded request carries Content-Length 9999
/// Clause: 4.3.6.5, 6.3.19
#[gtest]
#[tokio::test]
async fn n_fed_wrt_6403_a_context_source_info_content_length_cannot_override_the_real_one() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration = CSourceRegistration::builder(source.root_endpoint()?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id(vehicle_id.clone())])
            .property_names(["speed"])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::CreateEntity])
        .build()?;
    // The model refuses it (a registration contextSourceInfo), so `contextSourceInfo` is written as text.
    let context_source_info: RawJson = r#"[{"key": "Content-Length", "value": "9999"}]"#.parse()?;
    let document = UncheckedDocument::try_from(registration.render())?.set_member("contextSourceInfo", &context_source_info);
    let request = broker.unchecked(&RegisterContextSource { registration: &registration })?.document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    source
        .answer(Method::POST, "/ngsi-ld/v1/entities", source_answer::status(StatusCode::CREATED))
        .await?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 30)
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let forwarded = source.received_on(Method::POST, "/ngsi-ld/v1/entities").await?;
    expect_that!(forwarded, len(eq(1)));
    let forwarded_request =
        SourceRequest::new(Method::POST, "/ngsi-ld/v1/entities").header(HeaderCondition::Is(HeaderName::from_static("content-length"), "9999".to_owned()));
    let forwarded_2 = source.received_matching(&forwarded_request).await?;
    expect_that!(forwarded_2, len(eq(0)));
    Ok(())
}
