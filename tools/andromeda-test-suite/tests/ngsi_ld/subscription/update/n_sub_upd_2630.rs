//! An empty fragment answers 204 and changes nothing.

use andromeda_test_suite::{broker, namespace::Namespace, notification_sink::NotificationSink, nth::nth};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::NgsiLdMediaType,
    query::{EntitySelector, builders::SelectorTypeBuilder},
    subscription::{Endpoint, NotificationParams, Subscription, SubscriptionFragment},
};

/// Pins: Update Subscription with the empty Fragment {} as application/json answers 204 and leaves the subscription
/// as stored
/// Clause: 5.4, 5.8.2
/// Recorded: a Fragment is an IETF RFC 7396 Merge Patch, under which {} is valid and changes nothing
#[gtest]
#[tokio::test]
async fn n_sub_upd_2630_an_empty_fragment_answers_204_and_changes_nothing() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let subscription_id = namespace.subscription_id(1)?;
    let building_type = namespace.entity_type("Building")?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .description("before")
        .entities([building_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint("s1")?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let fragment = SubscriptionFragment::builder().build()?;
    broker.subscriptions().update(&subscription_id, &fragment).send().await?;
    let subscription_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_id)
        .send()
        .await?;
    expect_that!(subscription_2.description(), some(eq("before")));
    expect_that!(
        subscription_2.entities().iter().map(EntitySelector::selector_type).collect::<Vec<_>>(),
        nth(0, eq(&&SelectorTypeBuilder::from(&building_type).build()?))
    );
    Ok(())
}
