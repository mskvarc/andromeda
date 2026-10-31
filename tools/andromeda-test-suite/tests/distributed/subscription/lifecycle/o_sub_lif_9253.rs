//! An expired subscription deletes the copy placed at its source.

use andromeda_test_suite::{
    body_pattern::BodyPattern,
    broker,
    context_source::ContextSource,
    eventually::retried,
    namespace::Namespace,
    notification_sink::NotificationSink,
    source_answer,
    source_request::SourceRequest,
};
use chrono::{TimeDelta, Utc};
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode};
use ngsi_ld::{
    http::headers::Accept,
    model::{DateTime, NgsiLdMediaType},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
    subscription::{Endpoint, NotificationParams, Subscription, SubscriptionStatus},
};

/// Pins: A subscription whose reduced copy was placed at the source of a registration advertising createSubscription and deleteSubscription, and whose expiresAt then passes, reads back as expired and the source receives a DELETE of the subscription it holds for the copy, as a deletion of the subscription would send
/// Clause: 5.8.1.4, 5.8.5.4
#[gtest]
#[tokio::test]
async fn o_sub_lif_9253_an_expired_subscription_deletes_the_copy_placed_at_its_source() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let source = ContextSource::start().await?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let subscription_id = namespace.subscription_id(1)?;
    let subscription_remote_id = namespace.subscription_id("remote")?;
    source
        .answer_when(
            SourceRequest::new(Method::POST, "/a/ngsi-ld/v1/subscriptions")
                .body(BodyPattern::Contains(format!("Vehicle_{namespace}")))
                .priority(1),
            source_answer::text(
                StatusCode::CREATED,
                HeaderValue::from_static("application/json"),
                &format!("{{\"id\":\"urn:ngsi-ld:Subscription:{namespace}-remote\"}}"),
            )
            .insert_header("location", format!("/ngsi-ld/v1/subscriptions/urn:ngsi-ld:Subscription:{namespace}-remote")),
        )
        .await?;
    source
        .answer_when(
            SourceRequest::any_matching("/a/ngsi-ld/v1/subscriptions.*").priority(5),
            source_answer::status(StatusCode::NO_CONTENT),
        )
        .await?;
    let in_4_seconds: DateTime = (Utc::now() + TimeDelta::seconds(4)).format("%Y-%m-%dT%H:%M:%SZ").to_string().parse()?;
    let registration = CSourceRegistration::builder(source.endpoint("a")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::CreateSubscription, DistributedOperation::DeleteSubscription])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([vehicle_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint("sink")?).build()?).build()?)
        .expires_at(in_4_seconds)
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let creation = SourceRequest::new(Method::POST, "/a/ngsi-ld/v1/subscriptions").body(BodyPattern::Contains(format!("Vehicle_{namespace}")));
    let created = source.wait_for_matching(&creation, 1).await?;
    expect_that!(created, len(eq(1)));
    retried(async || {
        let stored = broker
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .subscriptions()
            .retrieve(&subscription_id)
            .send()
            .await?;
        verify_that!(stored.output().status, some(eq(SubscriptionStatus::Expired)))
    })
    .await?;
    let deleted = source
        .wait_for_on(Method::DELETE, &format!("/a/ngsi-ld/v1/subscriptions/{subscription_remote_id}"), 1)
        .await?;
    expect_that!(deleted, len(eq(1)));
    let stored_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_id)
        .send()
        .await?;
    expect_that!(stored_2.output().status, some(eq(SubscriptionStatus::Expired)));
    Ok(())
}
