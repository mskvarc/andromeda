//! Expand values json keys and q read back as written.

use andromeda_test_suite::{body_json::json_equals, broker, config::SuiteConfig, namespace::Namespace, problem::answered};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::CreateSubscription},
    model::{NgsiLdMediaType, RawJson},
    query::{EntitySelector, builders::SelectorTypeBuilder, q},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: A Subscription with q category=="commercial";address`[city]`=="Berlin", `expandValues` category and
/// `jsonKeys` address reads back those three members as written
/// Clause: 4.9, Table 5.2.12-1, 5.8.3
#[gtest]
#[tokio::test]
async fn n_sub_ret_9003_expand_values_json_keys_and_q_read_back_as_written() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let unreachable = config.unreachable_origin().origin().ascii_serialization();
    let subscription_id = namespace.subscription_id("opt-in")?;
    let building_id = namespace.urn("Building", "opt-in")?;
    let building_type = namespace.entity_type("Building")?;
    let query = q::attr("category")
        .eq("commercial")
        .and(q::attr("address").member("city").eq("Berlin"))
        .build()?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([EntitySelector::new(SelectorTypeBuilder::from(&building_type).build()?).with_ids(building_id)])
        .q(query.clone())
        .expand_values(["category"])
        .json_keys(["address"])
        .notification(NotificationParams::builder(Endpoint::builder(format!("{unreachable}/notify")).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    // The model writes expandValues and jsonKeys as arrays, and the subject is the member as a string, so those two are set as text.
    let category: RawJson = r#""category""#.parse()?;
    let address: RawJson = r#""address""#.parse()?;
    let document = UncheckedDocument::try_from(subscription.render())?
        .set_member("expandValues", &category)
        .set_member("jsonKeys", &address);
    let request = broker.unchecked(&CreateSubscription { subscription: &subscription })?.document(&document);
    let created = broker.send_unchecked(request).await;
    expect_that!(&created, answered(StatusCode::CREATED));
    let stored = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_id)
        .send()
        .await?;
    // The typed lists hold one name either way, so the string spelling is read from the body.
    expect_that!(&stored.body, json_equals("$.expandValues", "category"));
    expect_that!(&stored.body, json_equals("$.jsonKeys", "address"));
    let stored = stored.outcome;
    expect_that!(
        stored.expand_values().iter().map(ToString::to_string).collect::<Vec<_>>(),
        elements_are![eq("category")]
    );
    expect_that!(
        stored.json_keys().iter().map(ToString::to_string).collect::<Vec<_>>(),
        elements_are![eq("address")]
    );
    expect_that!(stored.q(), some(eq(&query)));
    expect_that!(
        stored.q().map(ToString::to_string),
        some(eq(r#"category=="commercial";address[city]=="Berlin""#))
    );
    broker.subscriptions().delete(&subscription_id).send().await?;
    Ok(())
}
