//! Watched attributes without entities notify any holder of the attribute.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, notification_sink::NotificationSink, nth::nth, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    model::{Entity, Lossless, NgsiLdMediaType},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: A subscription with `watchedAttributes` and no entities member is notified of the creation of an entity of
/// any type holding that attribute
/// Clause: 5.8.6, 5.8.1.4
/// Tenant: private (a subscription naming no entities matches every registration of its tenant (5.12), so in the
/// shared tenant it would be forwarded to every other file's context sources)
#[gtest]
#[tokio::test]
async fn n_ntf_trg_6800_watched_attributes_without_entities_notify_any_holder_of_the_attribute() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let tenant = broker::private_tenant(&broker, &namespace).await?;
    let subscription_id = namespace.subscription_id(1)?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .watched_attributes([namespace.attribute("speed")?.to_string()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    tenant.subscriptions().create(&subscription).send().await?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type)
        .property(namespace.attribute("speed")?, 33)
        .build()?;
    tenant.entities().create(&vehicle).send().await?.done()?;
    let notifications = sink.wait_for_at_least(1, 1).await?;
    let notification = notifications.first().or_fail()?.entities(Lossless::Normalized)?;
    expect_that!(notification.subscription_id(), eq(&subscription_id));
    expect_that!(notification.data().as_slice(), nth(0, has_id(&vehicle_id)));
    tenant.subscriptions().delete(&subscription_id).send().await?;
    Ok(())
}
