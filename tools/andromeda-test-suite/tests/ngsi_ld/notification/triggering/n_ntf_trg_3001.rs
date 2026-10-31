//! Id takes precedence over id pattern in a subscription entity selector.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, notification_sink::NotificationSink, nth::nth, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    model::{Entity, Lossless, NgsiLdMediaType, Representation},
    query::{EntitySelector, IdPattern, builders::SelectorTypeBuilder},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: a subscription whose entity selector gives both id and `idPattern` notifies the entity the id names
/// although the pattern does not match it, and does not notify an entity only the pattern matches
/// Clause: 5.2.33, 5.8.6, 5.2.12
///
/// Each entity carries an attribute: without `notificationTrigger` the default triggers are `attributeCreated` and
/// `attributeUpdated` (Table 5.2.12-1), which an attribute-less creation does not raise.
#[gtest]
#[tokio::test]
async fn n_ntf_trg_3001_id_takes_precedence_over_id_pattern_in_a_subscription_entity_selector() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let subscription_id = namespace.subscription_id(1)?;
    let building_type = namespace.entity_type("Building")?;
    let building_b1_id = namespace.urn("Building", "b1")?;
    let building_a1_id = namespace.urn("Building", "a1")?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([EntitySelector::new(SelectorTypeBuilder::from(&building_type).build()?)
            .with_ids(building_b1_id.clone())
            .with_id_pattern(format!("^urn:ngsi-ld:Building:{namespace}-a.*").parse::<IdPattern>()?)])
        .notification(
            NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?)
                .format(Representation::Normalized)
                .build()?,
        )
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    // the entity only the pattern matches is created first: it must send nothing, so the oldest notification is the id's
    let building_a1 = Entity::builder()
        .id(building_a1_id)
        .entity_type(building_type.clone())
        .property("name", "a1")
        .build()?;
    broker.entities().create(&building_a1).send().await?.done()?;
    let building_b1 = Entity::builder()
        .id(building_b1_id.clone())
        .entity_type(building_type.clone())
        .property("name", "b1")
        .build()?;
    broker.entities().create(&building_b1).send().await?.done()?;
    sink.wait_for_at_least(1, 1).await?;
    let notifications = sink.received(1).await?;
    let notification = notifications.first().or_fail()?.entities(Lossless::Normalized)?;
    expect_that!(notification.subscription_id(), eq(&subscription_id));
    expect_that!(notification.data().as_slice(), nth(0, has_id(&building_b1_id)));
    Ok(())
}
