//! An update no longer matching the registration deletes the copy.

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
    subscription::{Endpoint, NotificationParams, Subscription, SubscriptionFragment},
};

/// Pins: Update Subscription replacing entities with a type the registration does not cover answers 204 and deletes
/// the copy at its own id
/// Clause: 5.8.1.4, 5.8.2.4
#[gtest]
#[tokio::test]
async fn n_sub_upd_6804_an_update_no_longer_matching_the_registration_deletes_the_copy() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let source = ContextSource::start().await?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let subscription_id = namespace.subscription_id(1)?;
    let device_type = namespace.entity_type("Device")?;
    source
        .answer(Method::POST, "/s1/ngsi-ld/v1/subscriptions", source_answer::status(StatusCode::CREATED))
        .await?;
    source
        .answer_when(
            SourceRequest::any_matching("/s1/ngsi-ld/v1/subscriptions/.*"),
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
    let fragment = SubscriptionFragment::builder().entities([device_type.as_str()]).build()?;
    broker.subscriptions().update(&subscription_id, &fragment).send().await?;
    let forwarded_2 = source
        .wait_for_on(Method::DELETE, &format!("/s1/ngsi-ld/v1/subscriptions/{copy_s1}"), 1)
        .await?;
    expect_that!(forwarded_2, len(eq(1)));
    Ok(())
}
