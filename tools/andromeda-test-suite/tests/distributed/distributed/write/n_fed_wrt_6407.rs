//! An entity scope is removed for an older source.

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

/// Pins: Create Entity with a scope under an inclusive registration declaring `ngsildConformance` 1.0 is forwarded
/// once with its speed and without the scope
/// Clause: 4.3.6.8, 4.3.6.6
#[gtest]
#[tokio::test]
async fn n_fed_wrt_6407_an_entity_scope_is_removed_for_an_older_source() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let registration = CSourceRegistration::builder(source.root_endpoint()?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::CreateEntity])
        .build()?;
    // The model refuses it (a registration contextSourceInfo), so `contextSourceInfo` is written as text.
    let context_source_info: RawJson = r#"[{"key": "ngsildConformance", "value": "1.0"}]"#.parse()?;
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
        .id(vehicle_id)
        .entity_type(vehicle_type.clone())
        .scope("/Madrid")
        .property("speed", 10)
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let forwarded_request = SourceRequest::new(Method::POST, "/ngsi-ld/v1/entities")
        .body(BodyPattern::Selects(format!("$[?(@.id == 'urn:ngsi-ld:Vehicle:{namespace}-1')]").parse()?));
    let forwarded = source.received_matching(&forwarded_request).await?;
    expect_that!(forwarded, len(eq(1)));
    let forwarded_request_2 = SourceRequest::new(Method::POST, "/ngsi-ld/v1/entities")
        .body(BodyPattern::Selects(format!("$[?(@.id == 'urn:ngsi-ld:Vehicle:{namespace}-1')]").parse()?))
        .body(BodyPattern::Selects("$.speed.value".parse()?));
    let forwarded_2 = source.received_matching(&forwarded_request_2).await?;
    expect_that!(forwarded_2, len(eq(1)));
    let forwarded_request_3 = SourceRequest::new(Method::POST, "/ngsi-ld/v1/entities")
        .body(BodyPattern::Selects(format!("$[?(@.id == 'urn:ngsi-ld:Vehicle:{namespace}-1')]").parse()?))
        .body(BodyPattern::Selects("$.scope".parse()?));
    let forwarded_3 = source.received_matching(&forwarded_request_3).await?;
    expect_that!(forwarded_3, len(eq(0)));
    Ok(())
}
