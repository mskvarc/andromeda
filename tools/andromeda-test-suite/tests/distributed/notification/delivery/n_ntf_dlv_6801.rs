//! A notification posted as merge patch is refused and relays nothing.

use andromeda_test_suite::{
    broker,
    context_source::ContextSource,
    core_context::core_context,
    identity_matcher::has_id,
    namespace::Namespace,
    notification_sink::NotificationSink,
    nth::nth,
    problem::answered,
    relayed_copy::{accept_copies, notification_body, placed_copy_id, relay},
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::ContentType,
    model::{APPLICATION_LD_JSON, Entity, Lossless, NgsiLdMediaType},
    registration::{CSourceRegistration, EntityInfo, OperationGroup, RegistrationInfo, RegistrationMode},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: A Notification of the copy posted as application/merge-patch+json answers 415, and the notification posted
/// next as application/ld+json is the only one the subscriber is sent
/// Clause: 6.3.4, 5.8.6
#[gtest]
#[tokio::test]
async fn n_ntf_dlv_6801_a_notification_posted_as_merge_patch_is_refused_and_relays_nothing() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let source = ContextSource::start().await?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let refused_id = namespace.urn("Vehicle", "refused")?;
    let accepted_id = namespace.urn("Vehicle", "accepted")?;
    accept_copies(&source, "s1").await?;
    let registration = CSourceRegistration::builder(source.endpoint("s1")?)
        .id(namespace.registration_id(1)?)
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([OperationGroup::FederationOps])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    let subscription = Subscription::builder()
        .id(namespace.subscription_id(1)?)
        .entities([vehicle_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let copy_id = placed_copy_id(&source, "s1", &vehicle_type).await?;
    let refused = Entity::output_builder()
        .id(refused_id)
        .entity_type(vehicle_type.clone())
        .property("speed", 40)
        .build()?;
    let refused_body = notification_body(&namespace, &copy_id, vec![refused], core_context()?)?;
    let answer = relay(&broker, &ContentType::MergePatch.media_type(), &refused_body).await?;
    expect_that!(&answer, answered(StatusCode::UNSUPPORTED_MEDIA_TYPE));
    let accepted = Entity::output_builder()
        .id(accepted_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 41)
        .build()?;
    let accepted_body = notification_body(&namespace, &copy_id, vec![accepted], core_context()?)?;
    let answer_2 = relay(&broker, &APPLICATION_LD_JSON, &accepted_body).await?;
    expect_that!(&answer_2, answered(StatusCode::OK));
    let notifications = sink.wait_for_at_least(1, 1).await?;
    let notification = notifications.first().or_fail()?.entities(Lossless::Normalized)?;
    expect_that!(notification.data().as_slice(), nth(0, has_id(&accepted_id)));
    let received = sink.received(1).await?;
    expect_that!(received, len(eq(1)));
    Ok(())
}
