//! An inline join notification nests a chain of 1000 linked entities.

use andromeda_test_suite::{
    broker,
    identity_matcher::has_id,
    linked_chain::building_chain,
    linked_entity::{chain_entity, chain_length},
    namespace::Namespace,
    notification_sink::NotificationSink,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    model::{EntityFragment, Lossless, NgsiLdMediaType},
    query::{EntitySelector, Join, builders::SelectorTypeBuilder},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: A Subscription to the head of a chain of 1000 Relationships with join inline and `joinLevel` 1000 is
/// notified of an appended attribute with every one of the 1000 Linked Entities nested under the entity before it,
/// down to the last
/// Clause: 5.2.14.1, 4.5.23.1, 4.5.23.2, 5.8.6
/// Recorded: 4.5.23.1 asks a broker "to limit retrieval to avoid cascades of an excessive length" through
/// `joinLevel`, and Table 6.5.3.1-1 sets no upper bound on it; Andromeda walks any depth the client asks for, in a
/// notification too
#[gtest]
#[tokio::test]
async fn o_lnk_ntf_9001_an_inline_join_notification_nests_a_chain_of_1000_linked_entities() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let building_type = namespace.entity_type("Building")?;
    let building_0_id = namespace.urn("Building", 0)?;
    let building_last_id = namespace.urn("Building", 1000)?;
    let subscription_id = namespace.subscription_id(1)?;
    let chain = building_chain(&namespace, &building_type, 1000)?;
    for batch in chain.chunks(500) {
        broker.entities().batch_create(batch).send().await?.done()?;
    }
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([EntitySelector::new(SelectorTypeBuilder::from(&building_type).build()?).with_ids(building_0_id.clone())])
        .notification(
            NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?)
                .join(Join::Inline)
                .join_level(1000)
                .build()?,
        )
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let fragment = EntityFragment::builder().property("name", "Touched").build()?;
    broker.entities().append_attributes(&building_0_id, &fragment).send().await?.done()?;
    sink.wait_for_at_least(1, 1).await?;
    let notifications = sink.received(1).await?;
    expect_that!(notifications, len(eq(1)));
    let notification = notifications.first().or_fail()?.entities(Lossless::Normalized)?;
    let entity = notification.data().first().or_fail()?;
    expect_that!(chain_length(entity, "linkedTo"), eq(1000));
    expect_that!(chain_entity(entity, "linkedTo", 1000), some(has_id(&building_last_id)));
    broker.subscriptions().delete(&subscription_id).send().await?;
    Ok(())
}
