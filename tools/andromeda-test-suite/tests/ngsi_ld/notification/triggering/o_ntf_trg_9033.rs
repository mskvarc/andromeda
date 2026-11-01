//! A change committed before its Subscription stopped watching the changed attribute is still notified.

use andromeda_test_suite::{broker, namespace::Namespace, notification_sink::NotificationSink, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    model::{Entity, EntityFragment, NgsiLdMediaType},
    subscription::{Endpoint, NotificationParams, Subscription, SubscriptionFragment},
};
use std::time::Duration;
use wiremock::ResponseTemplate;

/// How long the endpoint holds each notification before it answers, under the broker's 10-second `request_timeout`.
/// The changes of one Entity are evaluated in commit order, each after the one before it was delivered, so the `seq`
/// update waits behind the held creation while the Subscription update lands.
const HELD: Duration = Duration::from_secs(3);

/// How long the endpoint is watched for a notification beyond the ones owed.
const QUIET_WINDOW: Duration = Duration::from_secs(3);

/// Pins: for a Vehicle created with seq 0 and other 0 under a Subscription watching `seq` whose endpoint holds each
/// notification 3 seconds, an update of seq to 1, an update of the Subscription to watch `other` instead, an update
/// of seq to 2 and an update of other to 1, the endpoint receives exactly three notifications: the creation, the
/// first seq update and the other update
/// Clause: 5.8.6, 5.8.2
/// Recorded: Andromeda reading of v1.9.1: clause 5.8.6 owes a notification for a change of a watched Attribute, and
/// clause 5.8.2.4 updates the Subscription without withdrawing what a change committed before it already owes; under
/// live delivery a change is evaluated after its commit, by the selection that stood at the commit
#[gtest]
#[tokio::test]
async fn o_ntf_trg_9033_a_change_committed_before_its_subscription_stopped_watching_the_attribute_is_still_notified() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    sink.answer("reselected", ResponseTemplate::new(204).set_delay(HELD)).await?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let subscription_id = namespace.subscription_id("reselected")?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([vehicle_type.as_str()])
        .watched_attributes(["seq"])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint("reselected")?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("seq", 0)
        .property("other", 0)
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let seq_watched = EntityFragment::builder().property("seq", 1).build()?;
    broker.entities().update_attributes(&vehicle_id, &seq_watched).send().await?.done()?;
    let reselected = SubscriptionFragment::builder().watched_attributes(["other"]).build()?;
    broker.subscriptions().update(&subscription_id, &reselected).send().await?;
    let seq_unwatched = EntityFragment::builder().property("seq", 2).build()?;
    broker.entities().update_attributes(&vehicle_id, &seq_unwatched).send().await?.done()?;
    let other_watched = EntityFragment::builder().property("other", 1).build()?;
    broker.entities().update_attributes(&vehicle_id, &other_watched).send().await?.done()?;
    sink.wait_for_at_least("reselected", 3).await?;
    expect_that!(sink.count_after("reselected", QUIET_WINDOW).await?, eq(3));
    broker.subscriptions().delete(&subscription_id).send().await?;
    Ok(())
}
