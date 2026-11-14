//! A context source filter does not filter which locally stored entities notify.

use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    notification_sink::{NotificationSink, SinkError},
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    model::{Entity, EntityId, Lossless, NgsiLdMediaType},
    query::q,
    subscription::{Endpoint, NotificationParams, Subscription},
};
use std::collections::HashSet;

/// Pins: a Subscription with csf tier=="silver" is notified of a locally stored Vehicle whose `tier` is gold and of
/// one whose `tier` is silver alike; a context source filter is met by Context Source Registrations, never by the
/// Entity's own Attributes, and outside a Context Source Registration Subscription it does not decide which
/// Entities notify
/// Clause: 5.2.12, 5.8.6, 4.9
/// Recorded: ETSI TS 104 175 V1.1.1 Table 5.2.6.5.2-1 says the filter is matched against the Context Source
/// Registration attributes when the Subscription serves a Context Source Registration Subscription, and "In other
/// cases it is ignored"; its clause 10.5.7 keeps the earlier sentence that only Entities whose origin Context Source
/// matches the filter are included, and Andromeda follows the table
#[gtest]
#[tokio::test]
async fn o_ntf_trg_9034_a_context_source_filter_does_not_filter_which_locally_stored_entities_notify() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let subscription_id = namespace.subscription_id("csf")?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([vehicle_type.as_str()])
        .csf(q::attr("tier").eq("silver").build()?)
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
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
    // A notification owed but never delivered shows as a missing Vehicle in the comparison, after the deadline.
    match sink.wait_for_at_least(1, 2).await {
        Ok(_) | Err(SinkError::TimedOut(_)) => {}
        Err(failure @ SinkError::Server(_)) => return Err(failure.into()),
    }
    let mut notified: HashSet<EntityId> = HashSet::new();
    for notification in sink.received(1).await? {
        notified.extend(notification.entities(Lossless::Normalized)?.data().iter().map(|entity| entity.id.clone()));
    }
    expect_that!(notified, eq(&created));
    broker.subscriptions().delete(&subscription_id).send().await?;
    Ok(())
}
