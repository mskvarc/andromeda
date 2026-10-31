//! An entity at the limit is served by every read.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::has_attribute,
    namespace::Namespace,
    nesting::{sub_attribute_chain, sub_attribute_chain_matcher},
    notification_sink::NotificationSink,
    nth::nth,
    outcome::Done,
    simplified_matcher::has_value,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::RepresentationSpelling},
    model::{Entity, EntityFragment, Lossless, NgsiLdMediaType, Representation},
    query::{EntitySelector, builders::SelectorTypeBuilder},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: An entity whose p carries a chain of 32 sub-Attributes is notified with the whole chain, answered whole by
/// Query Entities, rendered as p 1 by `keyValues`, and kept whole by an Update Attributes of another Attribute
/// Clause: 5.8.6, 5.7.2.4, 4.5.4, 5.6.2.4
#[gtest]
#[tokio::test]
async fn a_lim_dep_9012_an_entity_at_the_limit_is_served_by_every_read() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let subscription_id = namespace.subscription_id(1)?;
    let nesting_probe_id = namespace.urn("NestingProbe", 1)?;
    let nesting_probe_type = namespace.entity_type("NestingProbe")?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([EntitySelector::new(SelectorTypeBuilder::from(&nesting_probe_type).build()?).with_ids(nesting_probe_id.clone())])
        .notification(
            NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?)
                .format(Representation::Normalized)
                .build()?,
        )
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let nesting_probe = Entity::builder()
        .id(nesting_probe_id.clone())
        .entity_type(nesting_probe_type.clone())
        .property("p", sub_attribute_chain(32)?)
        .build()?;
    broker.entities().create(&nesting_probe).send().await?.done()?;
    sink.wait_for_at_least(1, 1).await?;
    let notifications = sink.received(1).await?;
    expect_that!(notifications, len(eq(1)));
    let notification = notifications.first().or_fail()?.entities(Lossless::Normalized)?;
    expect_that!(notification.data().as_slice(), nth(0, has_attribute("p", sub_attribute_chain_matcher(32))));
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&nesting_probe_type)
        .ids(vec![nesting_probe_id.clone()])
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_attribute("p", sub_attribute_chain_matcher(32))));
    let simplified = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve_simplified(&nesting_probe_id)
        .spelling(RepresentationSpelling::default().in_options().key_values())
        .send()
        .await?;
    expect_that!(simplified.body(), has_value("p", 1));
    let fragment = EntityFragment::builder().property("q", 2).build()?;
    broker.entities().update_attributes(&nesting_probe_id, &fragment).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&nesting_probe_id)
        .send()
        .await?;
    expect_that!(
        retrieved.body(),
        all!(
            has_attribute("p", sub_attribute_chain_matcher(32)),
            has_attribute("q", any_attribute().value(2))
        )
    );
    broker.subscriptions().delete(&subscription_id).send().await?;
    Ok(())
}
