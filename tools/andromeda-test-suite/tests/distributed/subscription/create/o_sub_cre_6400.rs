//! A forwarded subscription keeps its expiry.

use andromeda_test_suite::{
    body_pattern::BodyPattern,
    broker,
    cleanup::Cleanup,
    context_source::ContextSource,
    namespace::Namespace,
    notification_sink::NotificationSink,
    problem::answered,
    source_answer,
    source_request::SourceRequest,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::{UncheckedDocument, operation::RegisterContextSource},
    model::{NgsiLdMediaType, RawJson},
    registration::{CSourceRegistration, EntityInfo, OperationGroup, RegistrationInfo, RegistrationMode},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: Create Subscription with `expiresAt` under an inclusive registration advertising `federationOps` and
/// declaring `ngsildConformance` 1.0 is forwarded to the source as a Create Subscription carrying the same
/// `expiresAt`
/// Clause: 5.8.1.4, 4.3.6.8
/// Recorded: 4.3.6.8 amends Entity data only and 5.8.1.4 forwards a reduced copy of the Subscription without saying
/// which members survive; Andromeda keeps `expiresAt`
#[gtest]
#[tokio::test]
async fn o_sub_cre_6400_a_forwarded_subscription_keeps_its_expiry() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let source = ContextSource::start().await?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let subscription_id = namespace.subscription_id(1)?;
    let registration = CSourceRegistration::builder(source.root_endpoint()?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([OperationGroup::FederationOps])
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
        .answer(Method::POST, "/ngsi-ld/v1/subscriptions", source_answer::status(StatusCode::CREATED))
        .await?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([vehicle_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .expires_at("2099-01-01T00:00:00Z".parse()?)
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let forwarded_request = SourceRequest::new(Method::POST, "/ngsi-ld/v1/subscriptions").body(BodyPattern::Contains(format!("Vehicle_{namespace}")));
    let forwarded = source.wait_for_matching(&forwarded_request, 1).await?;
    expect_that!(forwarded, len(eq(1)));
    let forwarded_request_2 = SourceRequest::new(Method::POST, "/ngsi-ld/v1/subscriptions")
        .body(BodyPattern::Contains(format!("Vehicle_{namespace}")))
        .body(BodyPattern::Contains("2099-01-01T00:00:00Z".to_owned()));
    let forwarded_2 = source.received_matching(&forwarded_request_2).await?;
    expect_that!(forwarded_2, len(eq(1)));
    Ok(())
}
