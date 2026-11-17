//! A context source filter does not filter which locally stored entities a periodic notification carries.

use andromeda_test_suite::{broker, namespace::Namespace, notification_sink::NotificationSink, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    model::{Entity, EntityId, Lossless, NgsiLdMediaType},
    query::q,
    subscription::{Endpoint, NotificationParams, Subscription},
};
use std::collections::HashSet;

/// Pins: a Subscription with `timeInterval` 1 and csf tier=="silver", created right after a Vehicle whose `tier` is
/// gold and one whose `tier` is silver, notifies first with both Vehicles; a context source filter is met by Context
/// Source Registrations, never by the Entity's own Attributes, so it decides no Entity a periodic notification carries
/// Clause: 5.2.12, 5.8.6, 4.9
#[gtest]
#[tokio::test]
async fn o_ntf_trg_9035_a_context_source_filter_does_not_filter_which_entities_a_periodic_notification_carries() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let subscription_id = namespace.subscription_id("periodic-csf")?;
    let mut created = HashSet::new();
    for tier in ["gold", "silver"] {
        let id = namespace.urn("Vehicle", tier)?;
        let vehicle = Entity::builder()
            .id(id.clone())
            .entity_type(vehicle_type.clone())
            .property("tier", tier)
            .build()?;
        broker.entities().create(&vehicle).send().await?.done()?;
        created.insert(id);
    }
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([vehicle_type.as_str()])
        .time_interval(1)
        .csf(q::attr("tier").eq("silver").build()?)
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint("periodic")?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let notifications = sink.wait_for_at_least("periodic", 1).await?;
    let notified: HashSet<EntityId> = notifications
        .first()
        .or_fail()?
        .entities(Lossless::Normalized)?
        .data()
        .iter()
        .map(|entity| entity.id.clone())
        .collect();
    expect_that!(notified, eq(&created));
    broker.subscriptions().delete(&subscription_id).send().await?;
    Ok(())
}
