//! A fragment is merged.

use andromeda_test_suite::{broker, namespace::Namespace, notification_sink::NotificationSink, nth::nth};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::NgsiLdMediaType,
    query::{EntitySelector, builders::SelectorTypeBuilder},
    subscription::{Endpoint, NotificationParams, Subscription, SubscriptionFragment},
};

/// Pins: Update Context Source Registration Subscription on broker A with a description fragment answers 204, and
/// the description reads back with the entities kept
/// Clause: 5.11.3.4, 5.5.8
#[gtest]
#[tokio::test]
async fn n_css_upd_7600_a_fragment_is_merged() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let subscription_id = namespace.subscription_id(1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let subscription = Subscription::registration_builder()
        .id(subscription_id.clone())
        .entities([vehicle_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.registration_subscriptions().create(&subscription).send().await?;
    let fragment = SubscriptionFragment::registration_builder().description("merged description").build()?;
    broker.registration_subscriptions().update(&subscription_id, &fragment).send().await?;
    let subscription_2 = broker
        .with_accept(Accept::any())
        .registration_subscriptions()
        .retrieve(&subscription_id)
        .send()
        .await?;
    expect_that!(subscription_2.description(), some(eq("merged description")));
    expect_that!(
        subscription_2.entities().iter().map(EntitySelector::selector_type).collect::<Vec<_>>(),
        nth(0, eq(&&SelectorTypeBuilder::from(&vehicle_type).build()?))
    );
    Ok(())
}
