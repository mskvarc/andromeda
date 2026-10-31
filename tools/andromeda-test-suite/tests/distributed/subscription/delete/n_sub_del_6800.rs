//! Deleting a subscription deletes its copy on every supporting source.

use andromeda_test_suite::{
    body_pattern::BodyPattern,
    broker,
    cleanup::Cleanup,
    context_source::ContextSource,
    forwarded_body,
    namespace::Namespace,
    notification_sink::NotificationSink,
    source_answer,
    source_request::SourceRequest,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    model::NgsiLdMediaType,
    registration::{CSourceRegistration, EntityInfo, OperationGroup, RegistrationInfo, RegistrationMode},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: Delete Subscription over two sources holding `federationOps` answers 204 and sends each source a delete of
/// the copy it was given
/// Clause: 5.8.1.4, 5.8.5.4
#[gtest]
#[tokio::test]
async fn n_sub_del_6800_deleting_a_subscription_deletes_its_copy_on_every_supporting_source() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let source = ContextSource::start().await?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration_2_id = namespace.registration_id(2)?;
    let subscription_id = namespace.subscription_id(1)?;
    source
        .answer(Method::POST, "/s1/ngsi-ld/v1/subscriptions", source_answer::status(StatusCode::CREATED))
        .await?;
    source
        .answer_when(
            SourceRequest::any_matching("/s1/ngsi-ld/v1/subscriptions/.*"),
            source_answer::status(StatusCode::NO_CONTENT),
        )
        .await?;
    source
        .answer(Method::POST, "/s2/ngsi-ld/v1/subscriptions", source_answer::status(StatusCode::CREATED))
        .await?;
    source
        .answer_when(
            SourceRequest::any_matching("/s2/ngsi-ld/v1/subscriptions/.*"),
            source_answer::status(StatusCode::NO_CONTENT),
        )
        .await?;
    let registration = CSourceRegistration::builder(source.endpoint("s1")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([OperationGroup::FederationOps])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    let registration_2 = CSourceRegistration::builder(source.endpoint("s2")?)
        .id(registration_2_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([OperationGroup::FederationOps])
        .build()?;
    broker.registrations().register(&registration_2).send().await?;
    let _registration_2_cleanup = Cleanup::of(&broker).registration(&registration_2_id);
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([vehicle_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let forwarded_request = SourceRequest::new(Method::POST, "/s1/ngsi-ld/v1/subscriptions").body(BodyPattern::SelectsBelow(
        "$.entities".parse()?,
        format!("$[?(@.type == 'https://uri.etsi.org/ngsi-ld/default-context/Vehicle_{namespace}')]").parse()?,
    ));
    let forwarded = source.wait_for_matching(&forwarded_request, 1).await?;
    expect_that!(forwarded, len(eq(1)));
    let copy_s1 = forwarded_body::subscription(forwarded.first().or_fail()?)?.id().or_fail()?.clone();
    let forwarded_request_2 = SourceRequest::new(Method::POST, "/s2/ngsi-ld/v1/subscriptions").body(BodyPattern::SelectsBelow(
        "$.entities".parse()?,
        format!("$[?(@.type == 'https://uri.etsi.org/ngsi-ld/default-context/Vehicle_{namespace}')]").parse()?,
    ));
    let forwarded_2 = source.wait_for_matching(&forwarded_request_2, 1).await?;
    expect_that!(forwarded_2, len(eq(1)));
    let copy_s2 = forwarded_body::subscription(forwarded_2.first().or_fail()?)?.id().or_fail()?.clone();
    broker.subscriptions().delete(&subscription_id).send().await?;
    let forwarded_3 = source
        .wait_for_on(Method::DELETE, &format!("/s1/ngsi-ld/v1/subscriptions/{copy_s1}"), 1)
        .await?;
    expect_that!(forwarded_3, len(eq(1)));
    let forwarded_4 = source
        .wait_for_on(Method::DELETE, &format!("/s2/ngsi-ld/v1/subscriptions/{copy_s2}"), 1)
        .await?;
    expect_that!(forwarded_4, len(eq(1)));
    Ok(())
}
