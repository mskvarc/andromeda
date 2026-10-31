//! Deleting a subscription deletes the copy placed at its source.

use andromeda_test_suite::{
    body_pattern::BodyPattern,
    broker,
    cleanup::Cleanup,
    context_source::ContextSource,
    namespace::Namespace,
    notification_sink::NotificationSink,
    source_answer,
    source_request::SourceRequest,
};
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode};
use ngsi_ld::{
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: Delete Subscription of a subscription whose reduced copy was placed at the source of a registration
/// advertising `createSubscription` and `deleteSubscription` answers 204, and the source then receives a DELETE of
/// the subscription it holds for the copy
/// Clause: 5.8.5.4, 5.8.1.4
#[gtest]
#[tokio::test]
async fn n_sub_del_7000_deleting_a_subscription_deletes_the_copy_placed_at_its_source() -> Result<()> {
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
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([vehicle_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint("sink")?).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let forwarded_request = SourceRequest::new(Method::POST, "/a/ngsi-ld/v1/subscriptions").body(BodyPattern::Contains(format!("Vehicle_{namespace}")));
    let forwarded = source.wait_for_matching(&forwarded_request, 1).await?;
    expect_that!(forwarded, len(eq(1)));
    let forwarded_2 = source
        .received_on(Method::DELETE, &format!("/a/ngsi-ld/v1/subscriptions/{subscription_remote_id}"))
        .await?;
    expect_that!(forwarded_2, len(eq(0)));
    broker.subscriptions().delete(&subscription_id).send().await?;
    let forwarded_3 = source
        .wait_for_on(Method::DELETE, &format!("/a/ngsi-ld/v1/subscriptions/{subscription_remote_id}"), 1)
        .await?;
    expect_that!(forwarded_3, len(eq(1)));
    Ok(())
}
