//! Subscriptions forwarded to one domain and deleted at once while it notifies them are all deleted.

use andromeda_test_suite::{
    answered_status::AnsweredStatus,
    broker,
    cleanup::Cleanup,
    config::{BrokerRole, PeerRole},
    namespace::Namespace,
    notification_sink::NotificationSink,
    outcome::Done,
    problem::answered,
};
use futures::future::join_all;
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
    subscription::{Endpoint, NotificationParams, Subscription, SubscriptionId},
};

/// The Subscriptions created on the hub, and the Entities created on the domain.
const SUBSCRIPTIONS: u32 = 64;

/// Pins: 64 Subscriptions by type on broker A, over an inclusive registration of broker B supporting the
/// subscription operations, are each created with 201; 64 Entities of the type created on B are each created with
/// 201; the 64 Delete Subscription requests then sent at once while B notifies them are each answered 204, and
/// each Subscription then retrieves with 404
/// Clause: 5.8.1.4, 5.8.5, 5.8.6
#[gtest]
#[tokio::test]
async fn n_sub_cre_8007_subscriptions_forwarded_to_one_domain_and_deleted_at_once_while_it_notifies_them_are_all_deleted() -> Result<()> {
    let namespace = Namespace::fresh();
    let hub = broker::front()?;
    let domain = broker::client(BrokerRole::B)?;
    let sink = NotificationSink::start().await?;
    let building_type = namespace.entity_type("Building")?;
    let registration = CSourceRegistration::builder(broker::peer(PeerRole::B)?)
        .id(namespace.registration_id("b")?)
        .information([RegistrationInfo::builder()
            // clone: the information element owns its type
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
    hub.registrations().register(&registration).send().await?;
    let subscription_ids = (0..SUBSCRIPTIONS)
        .map(|number| Ok(namespace.subscription_id(number)?))
        .collect::<Result<Vec<SubscriptionId>>>()?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint. The Subscriptions
    // are removed before it, so a test that ends early leaves no copy notifying a sink that is gone.
    let _cleanup = subscription_ids
        .iter()
        .fold(Cleanup::of(&hub).registration_of(&registration), Cleanup::subscription);
    for subscription_id in &subscription_ids {
        let subscription = Subscription::builder()
            // clone: the Subscription owns its id, and the test deletes it by that id
            .id(subscription_id.clone())
            .entities([building_type.as_str()])
            .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
            .build()?;
        hub.subscriptions().create(&subscription).send().await?;
    }
    for number in 0..SUBSCRIPTIONS {
        let building = Entity::builder()
            .id(namespace.urn("Building", number)?)
            // clone: every Entity owns its type
            .entity_type(building_type.clone())
            .property("speed", number)
            .build()?;
        domain.entities().create(&building).send().await?.done()?;
    }
    let deleting = hub.with_heads();
    let deletions = join_all(
        subscription_ids
            .iter()
            .map(|subscription_id| async { deleting.subscriptions().delete(subscription_id).send().await }),
    )
    .await;
    expect_that!(
        deletions.iter().map(AnsweredStatus::answered_status).collect::<Vec<_>>(),
        each(some(eq(&StatusCode::NO_CONTENT)))
    );
    let retrieving = hub.with_responses().with_accept(Accept::new(NgsiLdMediaType::Json));
    for subscription_id in &subscription_ids {
        let gone = retrieving.subscriptions().retrieve(subscription_id).send().await;
        expect_that!(&gone, answered(StatusCode::NOT_FOUND));
    }
    Ok(())
}
