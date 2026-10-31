//! A cooldown drops the notifications of its window.

use andromeda_test_suite::{body_pattern::BodyPattern, broker, namespace::Namespace, notification_sink::NotificationSink, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    model::{Entity, EntityFragment, NgsiLdMediaType},
    query::{EntitySelector, builders::SelectorTypeBuilder},
    subscription::{Endpoint, NotificationParams, Subscription},
};
use std::time::Duration;
use tokio::time::sleep;
use wiremock::{
    Mock,
    ResponseTemplate,
    matchers::{method, path},
};

/// Pins: After a notification answered 500 to an endpoint with cooldown 8000, a change within the window is not
/// notified, and a change after it is
/// Clause: Table 5.2.15-1, 5.8.6
#[gtest]
#[tokio::test]
async fn n_ntf_dlv_9003_a_cooldown_drops_the_notifications_of_its_window() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let building_type = namespace.entity_type("Building")?;
    let building_id = namespace.urn("Building", "cooldown")?;
    let subscription_id = namespace.subscription_id("cooldown")?;
    // The first delivery fails, so the cooldown starts; every later one is answered by the sink's catch-all 200.
    sink.mount(
        Mock::given(method("POST"))
            .and(path("/notify/cooldown"))
            .respond_with(ResponseTemplate::new(500).set_body_raw("{}", "application/ld+json"))
            .up_to_n_times(1)
            .with_priority(1),
    )
    .await?;
    let building = Entity::builder().id(building_id.clone()).entity_type(building_type.clone()).build()?;
    broker.entities().create(&building).send().await?.done()?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([EntitySelector::new(SelectorTypeBuilder::from(&building_type).build()?).with_ids(building_id.clone())])
        .notification(
            NotificationParams::builder(
                Endpoint::builder(sink.endpoint("cooldown")?)
                    .accept(NgsiLdMediaType::Json)
                    .cooldown(8000)
                    .build()?,
            )
            .build()?,
        )
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let first = EntityFragment::builder().property("note", "cooldown-first").build()?;
    broker.entities().append_attributes(&building_id, &first).send().await?.done()?;
    sink.wait_for_at_least("cooldown", 1).await?;
    let second = EntityFragment::builder().property("note", "cooldown-second").build()?;
    broker.entities().append_attributes(&building_id, &second).send().await?.done()?;
    sleep(Duration::from_secs(9)).await;
    let third = EntityFragment::builder().property("note", "cooldown-third").build()?;
    broker.entities().append_attributes(&building_id, &third).send().await?.done()?;
    let third_notified = sink
        .wait_for_at_least_where("cooldown", &[BodyPattern::Contains("cooldown-third".to_owned())], 1)
        .await?;
    expect_that!(third_notified, not(is_empty()));
    expect_that!(sink.received("cooldown").await?, len(eq(2)));
    expect_that!(
        sink.received_where("cooldown", &[BodyPattern::Contains("cooldown-second".to_owned())]).await?,
        is_empty()
    );
    broker.subscriptions().delete(&subscription_id).send().await?;
    Ok(())
}
