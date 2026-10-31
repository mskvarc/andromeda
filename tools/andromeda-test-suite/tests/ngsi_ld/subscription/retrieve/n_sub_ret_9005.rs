//! Every member a client supplies reads back as written.

use andromeda_test_suite::{broker, config::SuiteConfig, namespace::Namespace};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{AttributeName, DateTime, NgsiLdMediaType, Representation},
    query::{builders::SelectorTypeBuilder, q},
    subscription::{Endpoint, NotificationParams, NotificationTrigger, Subscription},
};
use url::Url;

/// Pins: A Subscription with `subscriptionName`, description, entities, `watchedAttributes`, `notificationTrigger`,
/// q, throttling, `expiresAt` and a notification with attributes, format and endpoint reads each back as written
/// Clause: Table 5.2.12-1, 5.8.1, 5.8.2
#[gtest]
#[tokio::test]
async fn n_sub_ret_9005_every_member_a_client_supplies_reads_back_as_written() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let unreachable = config.unreachable_origin().origin().ascii_serialization();
    let subscription_id = namespace.subscription_id("round-trip")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let speed = "speed".parse::<AttributeName>()?;
    let query = q::attr("speed").gt(10).build()?;
    let expires_at = "2099-01-01T00:00:00Z".parse::<DateTime>()?;
    let endpoint_uri = format!("{unreachable}/notify").parse::<Url>()?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .subscription_name("typed round trip")
        .description("every member the client may supply")
        .entities([vehicle_type.as_str()])
        .watched_attributes(["speed"])
        .notification_triggers([NotificationTrigger::AttributeCreated, NotificationTrigger::AttributeUpdated])
        .q(query.clone())
        .throttling(5)
        .expires_at(expires_at)
        .notification(
            NotificationParams::builder(Endpoint::builder(endpoint_uri.clone()).accept(NgsiLdMediaType::Json).build()?)
                .attributes(["speed"])
                .format(Representation::Normalized)
                .build()?,
        )
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let stored = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_id)
        .send()
        .await?;
    expect_that!(stored.id(), eq(&subscription_id));
    expect_that!(stored.name(), some(eq("typed round trip")));
    expect_that!(stored.description(), some(eq("every member the client may supply")));
    expect_that!(stored.entities(), len(eq(1)));
    let selector = stored.entities().first().or_fail()?;
    expect_that!(selector.selector_type(), eq(&SelectorTypeBuilder::from(&vehicle_type).build()?));
    expect_that!(selector.ids(), none());
    expect_that!(stored.watched_attributes(), elements_are![eq(&speed)]);
    expect_that!(
        stored.notification_triggers().map(|triggers| triggers.iter().collect::<Vec<_>>()),
        some(elements_are![
            eq(&NotificationTrigger::AttributeCreated),
            eq(&NotificationTrigger::AttributeUpdated)
        ])
    );
    expect_that!(stored.q(), some(eq(&query)));
    expect_that!(stored.throttling().map(ToString::to_string), some(eq("5")));
    expect_that!(stored.expires_at(), some(eq(&expires_at)));
    let notification = stored.notification();
    expect_that!(notification.attributes(), elements_are![eq(&speed)]);
    expect_that!(notification.format(), some(eq(Representation::Normalized)));
    expect_that!(notification.endpoint().uri(), eq(&endpoint_uri));
    expect_that!(notification.endpoint().accept(), some(eq(NgsiLdMediaType::Json)));
    broker.subscriptions().delete(&subscription_id).send().await?;
    Ok(())
}
