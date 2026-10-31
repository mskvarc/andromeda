//! A context source info content type sets the forwarded content type to json.

use andromeda_test_suite::{
    broker,
    cleanup::Cleanup,
    context_source::ContextSource,
    forwarded_head,
    namespace::Namespace,
    outcome::Done,
    problem::answered,
    source_answer,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::{UncheckedDocument, headers::ContentType, operation::RegisterContextSource},
    model::{Entity, NgsiLdMediaType, RawJson},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Create Entity under an inclusive registration whose `contextSourceInfo` `contentType` is application/json
/// is forwarded once with Content-Type application/json
/// Clause: 4.3.6.6, 6.3.19, 5.6.1.4
#[gtest]
#[tokio::test]
async fn n_fed_wrt_6400_a_context_source_info_content_type_sets_the_forwarded_content_type_to_json() -> Result<()> {
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
    let context_source_info: RawJson = r#"[{"key": "contentType", "value": "application/json"}]"#.parse()?;
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
    let forwarded_one = forwarded.first().or_fail()?;
    expect_that!(
        forwarded_head::head(forwarded_one).typed::<ContentType>().valid(),
        some(eq(&ContentType::NgsiLd(NgsiLdMediaType::Json)))
    );
    Ok(())
}
