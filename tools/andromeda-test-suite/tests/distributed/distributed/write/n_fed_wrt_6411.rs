//! A forwarded partial attribute update loses members newer than the source version.

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
use cefact_units::UnitCode;
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::{UncheckedDocument, operation::RegisterContextSource},
    model::{Attribute, AttributeName, Entity, Lossless, Property, RawJson},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Partial Attribute Update of speed with value, `observedAt` and `unitCode` under an inclusive registration
/// declaring `ngsildConformance` 1.0 is forwarded once with value and without `observedAt` and `unitCode`
/// Clause: 4.3.6.8, 5.6.4.4
#[gtest]
#[tokio::test]
async fn n_fed_wrt_6411_a_forwarded_partial_attribute_update_loses_members_newer_than_the_source_version() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 1)
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let registration = CSourceRegistration::builder(source.root_endpoint()?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id(vehicle_id.clone())])
            .property_names(["speed"])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::UpdateAttrs])
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
        .answer(
            Method::PATCH,
            &format!("/ngsi-ld/v1/entities/{vehicle_id}/attrs/speed"),
            source_answer::status(StatusCode::NO_CONTENT),
        )
        .await?;
    let patch = Attribute::from(
        Property::builder(10)
            .unit_code(UnitCode::Kmh)
            .observed_at("2025-01-01T00:00:00Z".parse()?)
            .build()?,
    )
    .into_patch();
    broker
        .entities()
        .update_attribute(&vehicle_id, &"speed".parse::<AttributeName>()?, patch)
        .form(Lossless::Concise)
        .send()
        .await?
        .done()?;
    let forwarded_request =
        SourceRequest::new(Method::PATCH, &format!("/ngsi-ld/v1/entities/{vehicle_id}/attrs/speed")).body(BodyPattern::Selects("$.value".parse()?));
    let forwarded = source.received_matching(&forwarded_request).await?;
    expect_that!(forwarded, len(eq(1)));
    let forwarded_request_2 =
        SourceRequest::new(Method::PATCH, &format!("/ngsi-ld/v1/entities/{vehicle_id}/attrs/speed")).body(BodyPattern::Selects("$.observedAt".parse()?));
    let forwarded_2 = source.received_matching(&forwarded_request_2).await?;
    expect_that!(forwarded_2, len(eq(0)));
    let forwarded_request_3 =
        SourceRequest::new(Method::PATCH, &format!("/ngsi-ld/v1/entities/{vehicle_id}/attrs/speed")).body(BodyPattern::Selects("$.unitCode".parse()?));
    let forwarded_3 = source.received_matching(&forwarded_request_3).await?;
    expect_that!(forwarded_3, len(eq(0)));
    Ok(())
}
