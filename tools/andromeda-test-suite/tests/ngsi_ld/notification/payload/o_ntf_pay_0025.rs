//! Appends sent at once each notify their own flat join with the value they wrote.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker::{self, BrokerClient},
    entity_matcher::has_attribute,
    identity_matcher::has_id,
    namespace::Namespace,
    notification_sink::NotificationSink,
    nth::nth,
    outcome::Done,
};
use futures::future::join_all;
use googletest::prelude::*;
use ngsi_ld::{
    model::{Entity, EntityFragment, EntityId, Lossless, NgsiLdMediaType},
    query::{EntitySelector, Join, builders::SelectorTypeBuilder},
    subscription::{Endpoint, NotificationParams, Subscription, SubscriptionId},
};

/// The Building chains, each appended to once, all at the same moment.
const CHAINS: u32 = 8;

/// One Building chain: a Building located at a City located in a Country, and the flat-join Subscription on the
/// Building, notifying the sink endpoint numbered like the chain.
#[derive(Debug)]
struct Chain {
    /// The Building the Subscription selects and the append writes to.
    building: EntityId,
    /// The City the Building is located at.
    city: EntityId,
    /// The Country the City is located in.
    country: EntityId,
    /// The Subscription on the Building alone.
    subscription: SubscriptionId,
    /// The `name` value the append writes.
    written: String,
}

/// Creates chain `index` on `broker`: its Country, City and Building, and its Subscription notifying endpoint `index`
/// of `sink` with `join` flat and `joinLevel` 2.
async fn link_chain(broker: &BrokerClient, namespace: &Namespace, sink: &NotificationSink, index: u32) -> Result<Chain> {
    let chain = Chain {
        building: namespace.urn("Building", index)?,
        city: namespace.urn("City", index)?,
        country: namespace.urn("Country", index)?,
        subscription: namespace.subscription_id(index)?,
        written: format!("chain-{index}-once"),
    };
    let building_type = namespace.entity_type("Building")?;
    let country = Entity::builder()
        .id(chain.country.clone())
        .entity_type(namespace.entity_type("Country")?)
        .property("name", "Portugal")
        .build()?;
    broker.entities().create(&country).send().await?.done()?;
    let city = Entity::builder()
        .id(chain.city.clone())
        .entity_type(namespace.entity_type("City")?)
        .relationship("locatedIn", chain.country.clone())
        .build()?;
    broker.entities().create(&city).send().await?.done()?;
    let building = Entity::builder()
        .id(chain.building.clone())
        .entity_type(building_type.clone())
        .relationship("locatedAt", chain.city.clone())
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    let subscription = Subscription::builder()
        .id(chain.subscription.clone())
        .entities([EntitySelector::new(SelectorTypeBuilder::from(&building_type).build()?).with_ids(chain.building.clone())])
        .notification(
            NotificationParams::builder(Endpoint::builder(sink.endpoint(index)?).accept(NgsiLdMediaType::Json).build()?)
                .join(Join::Flat)
                .join_level(2)
                .build()?,
        )
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    Ok(chain)
}

/// Pins: eight flat-join Subscriptions with `joinLevel` 2, each on a Building of its own, notified of eight Append
/// Attributes sent at once, one to each Building, each notify once with their Building first, carrying the value its
/// own append wrote, then the City it links to, then the Country the City links to
/// Clause: 5.8.6, 4.5.23.3, 5.6.3
/// Recorded: clause 4.5.23.3 does not fix the order of the Entities a flat join returns; Andromeda delivers the
/// notified Entity first and the Linked Entities in the breadth-first order in which the traversal first reaches
/// them
#[gtest]
#[tokio::test]
async fn o_ntf_pay_0025_appends_sent_at_once_each_notify_their_own_flat_join_with_the_value_they_wrote() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let mut chains = Vec::new();
    for index in 0..CHAINS {
        chains.push(link_chain(&broker, &namespace, &sink, index).await?);
    }
    let chains = chains;
    let fragments = chains
        .iter()
        .map(|chain| Ok(EntityFragment::builder().property("name", chain.written.as_str()).build()?))
        .collect::<Result<Vec<_>>>()?;
    let entities = broker.entities();
    let appended = join_all(
        chains
            .iter()
            .zip(&fragments)
            .map(|(chain, fragment)| entities.append_attributes(&chain.building, fragment).send()),
    )
    .await;
    for outcome in appended {
        outcome?.done()?;
    }
    for (index, chain) in (0_u32..).zip(&chains) {
        let notifications = sink.wait_for_at_least(index, 1).await?;
        expect_that!(notifications, len(eq(1)));
        let notification = notifications.first().or_fail()?.entities(Lossless::Normalized)?;
        expect_that!(notification.subscription_id(), eq(&chain.subscription));
        expect_that!(notification.data().as_slice(), len(eq(3)));
        expect_that!(
            notification.data().as_slice(),
            nth(
                0,
                all!(has_id(&chain.building), has_attribute("name", any_attribute().value(chain.written.as_str())))
            )
        );
        expect_that!(notification.data().as_slice(), nth(1, has_id(&chain.city)));
        expect_that!(notification.data().as_slice(), nth(2, has_id(&chain.country)));
    }
    Ok(())
}
