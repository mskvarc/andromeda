//! A deletion notifies a subscription listening for entity updated.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, notification_sink::NotificationSink, nth::nth, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    model::{
        Entity,
        Lossless,
        NgsiLdMediaType,
        geometry::{LinearRing, Polygon, Position, Shape},
    },
    query::q,
    subscription::{Endpoint, NotificationParams, NotificationTrigger, Subscription},
};

/// Pins: Delete Entity notifies a Subscription whose only trigger is `entityUpdated` and whose q the removed state
/// passes, `entityUpdated` including `attributeDeleted`
/// Clause: Table 5.2.12-1, 5.8.6, 5.6.6
#[gtest]
#[tokio::test]
async fn n_ntf_trg_9002_a_deletion_notifies_a_subscription_listening_for_entity_updated() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let probe_updated_watcher_id = namespace.urn("Probe", "updated-watcher")?;
    let probe_type = namespace.entity_type("Probe")?;
    let subscription_updated_id = namespace.subscription_id("updated")?;
    let probe_updated_watcher = Entity::builder()
        .id(probe_updated_watcher_id.clone())
        .entity_type(probe_type.clone())
        .property("state", "retired")
        .geo_property(
            "location",
            Shape::Polygon(Polygon::try_new(vec![LinearRing::try_new(vec![
                Position::try_new(14.6, 46.05)?,
                Position::try_new(14.588_302_222, 46.082_139_38)?,
                Position::try_new(14.558_682_409, 46.099_240_388)?,
                Position::try_new(14.525, 46.093_301_27)?,
                Position::try_new(14.503_015_369, 46.067_101_007)?,
                Position::try_new(14.503_015_369, 46.032_898_993)?,
                Position::try_new(14.525, 46.006_698_73)?,
                Position::try_new(14.558_682_409, 46.000_759_612)?,
                Position::try_new(14.588_302_222, 46.017_860_62)?,
                Position::try_new(14.6, 46.05)?,
            ])?])?),
        )
        .build()?;
    broker.entities().create(&probe_updated_watcher).send().await?.done()?;
    let subscription_updated = Subscription::builder()
        .id(subscription_updated_id.clone())
        .entities([probe_type.as_str()])
        .notification_triggers([NotificationTrigger::EntityUpdated])
        .q(q::attr("state").eq("retired").build()?)
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint("updated")?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&subscription_updated).send().await?;
    broker.entities().delete(&probe_updated_watcher_id).send().await?.done()?;
    sink.wait_for_at_least("updated", 1).await?;
    let notifications = sink.received("updated").await?;
    let notification = notifications.first().or_fail()?.entities(Lossless::Normalized)?;
    expect_that!(notification.data().as_slice(), nth(0, has_id(&probe_updated_watcher_id)));
    broker.subscriptions().delete(&subscription_updated_id).send().await?;
    Ok(())
}
