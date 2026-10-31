//! A Subscription q that is a bare attribute tests existence.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::has_attribute,
    identity_matcher::has_id,
    namespace::Namespace,
    notification_sink::NotificationSink,
    nth::nth,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    model::{Entity, EntityFragment, Lossless, NgsiLdMediaType},
    query::q,
    subscription::{Endpoint, NotificationParams, Subscription},
};
use std::time::Duration;

/// How long the endpoint is watched for a notification beyond the one owed.
const QUIET_WINDOW: Duration = Duration::from_secs(3);

/// Pins: Create Subscription with q=n admits it; for an Entity created with m alone, then updated in m, then given
/// n, the Subscription receives one notification, and it holds n
/// Clause: 4.9, 5.2.12, 5.8.1.4, 5.8.6
/// Recorded: clause 4.9 makes an attribute path alone a Query Term that holds when the Entity defines the element,
/// and Table 5.2.12-1 types a Subscription q as that language
#[gtest]
#[tokio::test]
async fn n_sub_cre_9030_a_subscription_q_that_is_a_bare_attribute_tests_existence() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let thing_type = namespace.entity_type("Thing")?;
    let thing_id = namespace.urn("Thing", 1)?;
    let subscription_id = namespace.subscription_id("exists")?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([thing_type.as_str()])
        .q(q::attr("n").exists().build()?)
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint("exists")?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let thing = Entity::builder()
        .id(thing_id.clone())
        .entity_type(thing_type.clone())
        .property("m", 1)
        .build()?;
    broker.entities().create(&thing).send().await?.done()?;
    let fragment = EntityFragment::builder().property("m", 2).build()?;
    broker.entities().update_attributes(&thing_id, &fragment).send().await?.done()?;
    let fragment_2 = EntityFragment::builder().property("n", 5).build()?;
    broker.entities().append_attributes(&thing_id, &fragment_2).send().await?.done()?;
    sink.wait_for_at_least("exists", 1).await?;
    expect_that!(sink.count_after("exists", QUIET_WINDOW).await?, eq(1));
    let received = sink.received("exists").await?;
    let notification = received.first().or_fail()?.entities(Lossless::Normalized)?;
    expect_that!(notification.subscription_id(), eq(&subscription_id));
    expect_that!(
        notification.data().as_slice(),
        nth(0, all!(has_id(&thing_id), has_attribute("n", any_attribute().value(5))))
    );
    broker.subscriptions().delete(&subscription_id).send().await?;
    Ok(())
}
