//! A subscription and each item of its collection open with the context.

use andromeda_test_suite::{broker, namespace::Namespace, notification_sink::NotificationSink};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::NgsiLdMediaType,
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: Retrieve Subscription answered as application/ld+json opens with @context, and so does the item of each
/// page of Query Subscriptions
/// Clause: 6.3.6, 5.8.3, 5.8.4
/// Recorded: 6.3.6 requires the @context in an application/ld+json body and fixes no member order (RFC 8259 objects
/// are unordered); Andromeda writes it as the first member, of every item of a collection too
#[gtest]
#[tokio::test]
async fn o_cmn_cnt_9001_a_subscription_and_each_item_of_its_collection_open_with_the_context() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let subscription_id = namespace.subscription_id(1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let subscription_2_id = namespace.subscription_id(2)?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([vehicle_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let subscription_2 = Subscription::builder()
        .id(subscription_2_id.clone())
        .entities([vehicle_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(2)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&subscription_2).send().await?;
    let ld_json = broker.with_responses().with_accept(Accept::new(NgsiLdMediaType::JsonLd));
    let retrieved = ld_json.subscriptions().retrieve(&subscription_id).send().await?;
    expect_that!(&String::from_utf8_lossy(&retrieved.body).into_owned(), contains_regex(r#"^\s*\{\s*"@context""#));
    // One item per page: the collection is the tenant's, and each page shows how one item opens.
    let page = ld_json.subscriptions().query().limit(1).offset(0).send().await?;
    expect_that!(page.outcome.items(), len(eq(1)));
    expect_that!(&String::from_utf8_lossy(&page.body).into_owned(), contains_regex(r#"^\s*\[\s*\{\s*"@context""#));
    let page_2 = ld_json.subscriptions().query().limit(1).offset(1).send().await?;
    expect_that!(page_2.outcome.items(), len(eq(1)));
    expect_that!(
        &String::from_utf8_lossy(&page_2.body).into_owned(),
        contains_regex(r#"^\s*\[\s*\{\s*"@context""#)
    );
    broker.subscriptions().delete(&subscription_id).send().await?;
    broker.subscriptions().delete(&subscription_2_id).send().await?;
    Ok(())
}
