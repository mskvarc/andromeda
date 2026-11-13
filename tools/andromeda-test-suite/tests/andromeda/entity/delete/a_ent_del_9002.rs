//! The expiry sweep deletes an entity as a deletion does.

use andromeda_test_suite::{
    broker,
    deleted_instance::deleted_instance,
    evolution_matcher::has_evolution,
    identity_matcher::has_id,
    instant_ahead::seconds_ahead,
    namespace::Namespace,
    notification_sink::NotificationSink,
    nth::nth,
    outcome::Done,
    system_time::{self, SystemMember},
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{DateTime, Entity, Lossless, NgsiLdMediaType},
    subscription::{Endpoint, NotificationParams, NotificationTrigger, Subscription},
};

/// Pins: An Entity created with speed 30 and an `expiresAt` two seconds ahead is deleted by the expiry sweep: a
/// Subscription listening to `entityDeleted` on its type is notified with the Entity carrying `deletedAt`, and its
/// Temporal Evolution records the deletion of speed as the NGSI-LD Null with a `deletedAt`
/// Clause: 4.22, 5.6.6.4, 5.8.6, 4.3.1
/// Recorded: Andromeda configuration, not v1.9.1: clause 4.22 lets a broker remove an expired Entity at a lag it
/// leaves open; Andromeda removes it through the deletion Delete Entity runs, and the stack sweeps every second
#[gtest]
#[tokio::test]
async fn a_ent_del_9002_the_expiry_sweep_deletes_an_entity_as_a_deletion_does() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_expiring_id = namespace.urn("Vehicle", "expiring")?;
    let subscription_id = namespace.subscription_id("deleted")?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([vehicle_type.as_str()])
        .notification_triggers([NotificationTrigger::EntityDeleted])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint("deleted")?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let in_2_seconds: DateTime = seconds_ahead(2).parse()?;
    let vehicle_expiring = Entity::builder()
        .id(vehicle_expiring_id.clone())
        .entity_type(vehicle_type)
        .property("speed", 30)
        .expires_at(in_2_seconds)
        .build()?;
    broker.entities().create(&vehicle_expiring).send().await?.done()?;
    let notifications = sink.wait_for_at_least("deleted", 1).await?;
    let notification = notifications.first().or_fail()?.entities(Lossless::Normalized)?;
    expect_that!(notification.data().as_slice(), nth(0, has_id(&vehicle_expiring_id)));
    let item = notification.data().first().or_fail()?;
    expect_that!(system_time::of_entity(item, SystemMember::DeletedAt).ok(), some(anything()));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_expiring_id)
        .send()
        .await?;
    expect_that!(retrieved.body(), has_evolution("speed", contains(deleted_instance())));
    broker.subscriptions().delete(&subscription_id).send().await?;
    Ok(())
}
