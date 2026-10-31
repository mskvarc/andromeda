//! A subscription is notified of an entity held two registrations away.

use andromeda_test_suite::{
    body_pattern::BodyPattern,
    broker,
    config::{BrokerRole, PeerRole},
    identity_matcher::has_id,
    namespace::Namespace,
    notification_sink::NotificationSink,
    nth::nth,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    model::{Entity, Lossless, NgsiLdMediaType},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: A Subscription on broker A by type, under an inclusive registration of broker D supporting the
/// subscription operations, which itself holds such a registration of broker C, notifies the subscriber under its
/// own id of an Entity created on C
/// Clause: 5.8.1.4, 4.3.6.4
#[gtest]
#[tokio::test]
async fn n_sub_cre_8002_a_subscription_is_notified_of_an_entity_held_two_registrations_away() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let broker_c = broker::client(BrokerRole::C)?;
    let broker_d = broker::client(BrokerRole::D)?;
    let sink = NotificationSink::start().await?;
    let building_type = namespace.entity_type("Building")?;
    let subscription_id = namespace.subscription_id(1)?;
    let building_c_id = namespace.urn("Building", "c")?;
    for (holder, registration_suffix, peer) in [(&broker_d, "d", PeerRole::C), (&broker, "a", PeerRole::D)] {
        let registration = CSourceRegistration::builder(broker::peer(peer)?)
            .id(namespace.registration_id(registration_suffix)?)
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
        holder.registrations().register(&registration).send().await?;
    }
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([building_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let building = Entity::builder()
        .id(building_c_id.clone())
        .entity_type(building_type)
        .property("speed", 32)
        .build()?;
    broker_c.entities().create(&building).send().await?.done()?;
    let notifications = sink.wait_for_at_least_where(1, &[BodyPattern::Contains(building_c_id.to_string())], 1).await?;
    expect_that!(notifications, len(eq(1)));
    let notification = notifications.first().or_fail()?.entities(Lossless::Normalized)?;
    expect_that!(notification.subscription_id(), eq(&subscription_id));
    expect_that!(notification.data(), len(eq(1)));
    expect_that!(notification.data().as_slice(), nth(0, has_id(&building_c_id)));
    Ok(())
}
