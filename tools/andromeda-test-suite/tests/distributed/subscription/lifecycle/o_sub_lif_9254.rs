//! An expired subscription given a future expiry places its copy again.

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
use chrono::{SubsecRound, TimeDelta, Utc};
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode};
use ngsi_ld::{
    http::headers::Accept,
    model::{DateTime, NgsiLdMediaType},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
    subscription::{Endpoint, NotificationParams, Subscription, SubscriptionFragment, SubscriptionStatus},
};

/// Pins: A subscription whose expiry deleted the reduced copy placed at the source of a registration advertising createSubscription and deleteSubscription, then updated with a future expiresAt, reads back as active and the source receives a second creation of the copy
/// Clause: 5.8.2.4, 5.8.1.4
#[gtest]
#[tokio::test]
async fn o_sub_lif_9254_an_expired_subscription_given_a_future_expiry_places_its_copy_again() -> Result<()> {
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
    let registration = CSourceRegistration::builder(source.endpoint("a")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::CreateSubscription, DistributedOperation::DeleteSubscription])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Taken once the Registration is answered, which can take seconds under load, so the expiry lies 4 s ahead of
    // the create; milliseconds are kept, since a clause 4.6.3 DateTime holds no more than microseconds.
    let in_4_seconds = DateTime::try_from((Utc::now() + TimeDelta::seconds(4)).trunc_subsecs(3))?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([vehicle_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint("sink")?).build()?).build()?)
        .expires_at(in_4_seconds)
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let creation = SourceRequest::new(Method::POST, "/a/ngsi-ld/v1/subscriptions").body(BodyPattern::Contains(format!("Vehicle_{namespace}")));
    let in_an_hour = DateTime::try_from((Utc::now() + TimeDelta::seconds(3600)).trunc_subsecs(3))?;
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
    let fragment = SubscriptionFragment::builder().expires_at(in_an_hour).build()?;
    broker.subscriptions().update(&subscription_id, &fragment).send().await?;
    let stored_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_id)
        .send()
        .await?;
    expect_that!(stored_2.output().status, some(eq(SubscriptionStatus::Active)));
    let recreated = source.wait_for_matching(&creation, 2).await?;
    expect_that!(recreated, len(eq(2)));
    Ok(())
}
