//! A subscription over two peer registrations is notified from either under its own id.

use andromeda_test_suite::{
    body_pattern::BodyPattern,
    broker,
    config::{BrokerRole, PeerRole},
    identity_matcher::has_id,
    namespace::Namespace,
    notification_sink::NotificationSink,
    nth::nth,
    outcome::Done,
    problem::answered,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, Lossless, NgsiLdMediaType},
    query::{EntitySelector, builders::SelectorTypeBuilder},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
    subscription::{Endpoint, NotificationParams, Subscription, SubscriptionFragment},
};

/// Pins: A Subscription on broker A by type, over inclusive registrations of brokers B and C supporting the
/// subscription operations, notifies the subscriber under its own id of an Entity created on B and of one created
/// on C; it is then updated and deleted with 204
/// Clause: 5.8.1.4, 5.8.2, 5.8.5, 5.8.6
#[gtest]
#[tokio::test]
async fn n_sub_cre_8000_a_subscription_over_two_peer_registrations_is_notified_from_either_under_its_own_id() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let broker_b = broker::client(BrokerRole::B)?;
    let broker_c = broker::client(BrokerRole::C)?;
    let sink = NotificationSink::start().await?;
    let building_type = namespace.entity_type("Building")?;
    let subscription_id = namespace.subscription_id(1)?;
    let building_b_id = namespace.urn("Building", "b")?;
    let building_c_id = namespace.urn("Building", "c")?;
    for (suffix, peer) in [("b", PeerRole::B), ("c", PeerRole::C)] {
        let registration = CSourceRegistration::builder(broker::peer(peer)?)
            .id(namespace.registration_id(suffix)?)
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
    }
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([building_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    for (client, id, speed) in [(&broker_b, &building_b_id, 30), (&broker_c, &building_c_id, 31)] {
        let building = Entity::builder()
            .id(id.clone())
            .entity_type(building_type.clone())
            .property("speed", speed)
            .build()?;
        client.entities().create(&building).send().await?.done()?;
    }
    for id in [&building_b_id, &building_c_id] {
        let notifications = sink.wait_for_at_least_where(1, &[BodyPattern::Contains(id.to_string())], 1).await?;
        expect_that!(notifications, len(eq(1)));
        let notification = notifications.first().or_fail()?.entities(Lossless::Normalized)?;
        expect_that!(notification.subscription_id(), eq(&subscription_id));
        expect_that!(notification.data(), len(eq(1)));
        expect_that!(notification.data().as_slice(), nth(0, has_id(id)));
    }
    let fragment = SubscriptionFragment::builder()
        .entities([EntitySelector::new(SelectorTypeBuilder::from(&building_type).build()?).with_id_pattern(namespace.id_pattern_ending("Building", "never")?)])
        .build()?;
    broker.subscriptions().update(&subscription_id, &fragment).send().await?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_id)
        .send()
        .await?;
    expect_that!(
        retrieved.entities().first().and_then(EntitySelector::id_pattern),
        some(eq(&namespace.id_pattern_ending("Building", "never")?))
    );
    broker.subscriptions().delete(&subscription_id).send().await?;
    let gone = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_id)
        .send()
        .await;
    expect_that!(&gone, answered(StatusCode::NOT_FOUND));
    Ok(())
}
