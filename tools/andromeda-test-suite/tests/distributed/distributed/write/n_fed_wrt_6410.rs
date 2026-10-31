//! A scope is removed for version 1 3 and kept for version 1 4.

use andromeda_test_suite::{
    body_pattern::BodyPattern,
    broker,
    cleanup::Cleanup,
    context_source::ContextSource,
    namespace::Namespace,
    outcome::Done,
    problem::answered,
    source_answer,
    source_request::SourceRequest,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::{UncheckedDocument, operation::RegisterContextSource},
    model::{Entity, RawJson},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Create Entity with a scope under inclusive registrations declaring `ngsildConformance` 1.3 and 1.4 is
/// forwarded to the first without the scope and to the second with it
/// Clause: 4.3.6.8, 4.3.6.6
#[gtest]
#[tokio::test]
async fn n_fed_wrt_6410_a_scope_is_removed_for_version_1_3_and_kept_for_version_1_4() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration_2_id = namespace.registration_id(2)?;
    let vehicle_2_id = namespace.urn("Vehicle", 2)?;
    let registration = CSourceRegistration::builder(source.endpoint("1")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id(vehicle_id.clone())])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::CreateEntity])
        .build()?;
    // The model refuses it (a registration contextSourceInfo), so `contextSourceInfo` is written as text.
    let context_source_info: RawJson = r#"[{"key": "ngsildConformance", "value": "1.3"}]"#.parse()?;
    let document = UncheckedDocument::try_from(registration.render())?.set_member("contextSourceInfo", &context_source_info);
    let request = broker.unchecked(&RegisterContextSource { registration: &registration })?.document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    let registration_2 = CSourceRegistration::builder(source.endpoint("2")?)
        .id(registration_2_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id(vehicle_2_id.clone())])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::CreateEntity])
        .build()?;
    // The model refuses it (a registration contextSourceInfo), so `contextSourceInfo` is written as text.
    let context_source_info_2: RawJson = r#"[{"key": "ngsildConformance", "value": "1.4"}]"#.parse()?;
    let document_2 = UncheckedDocument::try_from(registration_2.render())?.set_member("contextSourceInfo", &context_source_info_2);
    let request_2 = broker
        .unchecked(&RegisterContextSource { registration: &registration_2 })?
        .document(&document_2);
    let answer_2 = broker.send_unchecked(request_2).await;
    expect_that!(&answer_2, answered(StatusCode::CREATED));
    let _registration_2_cleanup = Cleanup::of(&broker).registration(&registration_2_id);
    source
        .answer(Method::POST, "/1/ngsi-ld/v1/entities", source_answer::status(StatusCode::CREATED))
        .await?;
    source
        .answer(Method::POST, "/2/ngsi-ld/v1/entities", source_answer::status(StatusCode::CREATED))
        .await?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .scope("/Madrid")
        .property("speed", 30)
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let vehicle_2 = Entity::builder()
        .id(vehicle_2_id.clone())
        .entity_type(vehicle_type.clone())
        .scope("/Madrid")
        .property("speed", 30)
        .build()?;
    broker.entities().create(&vehicle_2).send().await?.done()?;
    let forwarded_request = SourceRequest::new(Method::POST, "/1/ngsi-ld/v1/entities").body(BodyPattern::Selects("$.speed.value".parse()?));
    let forwarded = source.received_matching(&forwarded_request).await?;
    expect_that!(forwarded, len(eq(1)));
    let forwarded_request_2 = SourceRequest::new(Method::POST, "/1/ngsi-ld/v1/entities").body(BodyPattern::Selects("$.scope".parse()?));
    let forwarded_2 = source.received_matching(&forwarded_request_2).await?;
    expect_that!(forwarded_2, len(eq(0)));
    let forwarded_request_3 = SourceRequest::new(Method::POST, "/2/ngsi-ld/v1/entities")
        .body(BodyPattern::Selects("$.speed.value".parse()?))
        .body(BodyPattern::Selects("$.scope".parse()?));
    let forwarded_3 = source.received_matching(&forwarded_request_3).await?;
    expect_that!(forwarded_3, len(eq(1)));
    Ok(())
}
