//! A subscription id pattern matches anywhere in the identifier unless anchored.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, notification_sink::NotificationSink, nth::nth, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    model::{Entity, Lossless, NgsiLdMediaType, Representation},
    query::{EntitySelector, IdPattern, builders::SelectorTypeBuilder},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: a subscription whose entity selector holds the `idPattern` `Car:<ns>-1` is notified of the creation of
/// `urn:ngsi-ld:Car:<ns>-12` and of `urn:ngsi-ld:Car:<ns>-1`, and one whose `idPattern` is `Car:<ns>-1$` is notified
/// of `urn:ngsi-ld:Car:<ns>-1` only
/// Clause: 5.2.33, 5.8.6, 5.2.12
///
/// The pattern is an IEEE 1003.2 regular expression, whose matching searches a string for a matching sequence, and
/// `$` limits the match to the end of the identifier. Each Entity carries an attribute: without
/// `notificationTrigger` the default triggers are `attributeCreated` and `attributeUpdated` (Table 5.2.12-1), which
/// an attribute-less creation does not raise.
#[gtest]
#[tokio::test]
async fn n_ntf_trg_9043_a_subscription_id_pattern_matches_anywhere_in_the_identifier_unless_anchored() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let car_type = namespace.entity_type("Car")?;
    let car_1_id = namespace.urn("Car", 1)?;
    let car_12_id = namespace.urn("Car", 12)?;
    for (n, pattern) in [(1, format!("Car:{namespace}-1")), (2, format!("Car:{namespace}-1$"))] {
        let subscription = Subscription::builder()
            .id(namespace.subscription_id(n)?)
            .entities([EntitySelector::new(SelectorTypeBuilder::from(&car_type).build()?).with_id_pattern(pattern.parse::<IdPattern>()?)])
            .notification(
                NotificationParams::builder(Endpoint::builder(sink.endpoint(n)?).accept(NgsiLdMediaType::Json).build()?)
                    .format(Representation::Normalized)
                    .build()?,
            )
            .build()?;
        broker.subscriptions().create(&subscription).send().await?;
    }
    // `-12` is created first, so a notification of it to endpoint 2 would be the oldest that endpoint holds.
    let car_12 = Entity::builder()
        .id(car_12_id.clone())
        .entity_type(car_type.clone())
        .property("name", "twelve")
        .build()?;
    broker.entities().create(&car_12).send().await?.done()?;
    let car_1 = Entity::builder()
        .id(car_1_id.clone())
        .entity_type(car_type.clone())
        .property("name", "one")
        .build()?;
    broker.entities().create(&car_1).send().await?.done()?;
    // Notifications of two Entities arrive in either order.
    sink.wait_for_at_least(1, 2).await?;
    let notifications = sink.received(1).await?;
    expect_that!(notifications, len(eq(2)));
    let mut notified = Vec::new();
    for received in &notifications {
        notified.extend(received.entities(Lossless::Normalized)?.into_data());
    }
    let notified = notified;
    expect_that!(notified.as_slice(), contains(has_id(&car_1_id)));
    expect_that!(notified.as_slice(), contains(has_id(&car_12_id)));
    sink.wait_for_at_least(2, 1).await?;
    let notifications_2 = sink.received(2).await?;
    expect_that!(notifications_2, len(eq(1)));
    let notification_2 = notifications_2.first().or_fail()?.entities(Lossless::Normalized)?;
    expect_that!(notification_2.data().as_slice(), len(eq(1)));
    expect_that!(notification_2.data().as_slice(), nth(0, has_id(&car_1_id)));
    Ok(())
}
