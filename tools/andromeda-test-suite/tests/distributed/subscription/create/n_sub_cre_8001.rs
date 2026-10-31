//! A registration created after the subscription receives its copy and relays its notifications.

use andromeda_test_suite::{
    body_pattern::BodyPattern,
    broker,
    config::{BrokerRole, PeerRole},
    eventually::retried,
    identity_matcher::has_id,
    namespace::Namespace,
    notification_sink::NotificationSink,
    nth::nth,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, Lossless, NgsiLdMediaType},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: A Subscription on broker A by type, created before an inclusive registration of broker B supporting the
/// subscription operations, is placed on broker B once the registration exists, and an Entity then created on B
/// notifies the subscriber under the id of the Subscription
/// Clause: 5.8.1.4, 5.11.7
#[gtest]
#[tokio::test]
async fn n_sub_cre_8001_a_registration_created_after_the_subscription_receives_its_copy_and_relays_its_notifications() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let broker_b = broker::client(BrokerRole::B)?;
    let sink = NotificationSink::start().await?;
    let building_type = namespace.entity_type("Building")?;
    let subscription_id = namespace.subscription_id(1)?;
    let building_b_id = namespace.urn("Building", "b")?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([building_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let registration = CSourceRegistration::builder(broker::peer(PeerRole::B)?)
        .id(namespace.registration_id("b")?)
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(building_type.clone()).with_id_pattern(namespace.id_pattern("Building")?)])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([
            DistributedOperation::QueryEntity,
            DistributedOperation::CreateSubscription,
            DistributedOperation::UpdateSubscription,
            DistributedOperation::DeleteSubscription,
        ])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    retried(async || {
        let placed = broker_b
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .subscriptions()
            .query()
            .limit(1000)
            .send()
            .await?;
        verify_that!(
            placed
                .items()
                .iter()
                .filter(|candidate| candidate
                    .entities()
                    .first()
                    .is_some_and(|selector| selector.render().to_string().contains(building_type.as_str())))
                .count(),
            ge(1)
        )?;
        Ok(())
    })
    .await?;
    let building = Entity::builder()
        .id(building_b_id.clone())
        .entity_type(building_type.clone())
        .property("speed", 31)
        .build()?;
    broker_b.entities().create(&building).send().await?.done()?;
    let notifications = sink.wait_for_at_least_where(1, &[BodyPattern::Contains(building_b_id.to_string())], 1).await?;
    expect_that!(notifications, len(eq(1)));
    let notification = notifications.first().or_fail()?.entities(Lossless::Normalized)?;
    expect_that!(notification.subscription_id(), eq(&subscription_id));
    expect_that!(notification.data(), len(eq(1)));
    expect_that!(notification.data().as_slice(), nth(0, has_id(&building_b_id)));
    Ok(())
}
