//! A client supplied times failed is ignored.

use andromeda_test_suite::{broker, config::SuiteConfig, namespace::Namespace, problem::answered, subscription_document::create_request};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::NgsiLdMediaType,
    query::{EntitySelector, builders::SelectorTypeBuilder},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: Create Subscription whose notification carries `timesFailed` 99 answers 201, and the Subscription reads
/// back with no `timesFailed` and no `timesSent`
/// Clause: Table 5.2.14.2-1, 5.8.1.4
/// Recorded: Table 5.2.14.2-1 makes `timesFailed` a member the system provides and does not say what becomes of one
/// a client sends; Andromeda accepts the Subscription and ignores the member
#[gtest]
#[tokio::test]
async fn o_sub_cre_9014_a_client_supplied_times_failed_is_ignored() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let unreachable = config.unreachable_origin().origin().ascii_serialization();
    let subscription_id = namespace.subscription_id("supplied")?;
    let building_id = namespace.urn("Building", "supplied")?;
    let building_type = namespace.entity_type("Building")?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([EntitySelector::new(SelectorTypeBuilder::from(&building_type).build()?).with_ids(building_id)])
        .notification(NotificationParams::builder(Endpoint::builder(format!("{unreachable}/notify")).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    // The notification model writes no timesFailed, so the member is added to the written body.
    let created = broker.send_unchecked(create_request(&broker, &subscription, &[("timesFailed", "99")])?).await;
    expect_that!(&created, answered(StatusCode::CREATED));
    let stored = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_id)
        .send()
        .await?;
    expect_that!(stored.notification().output().times_failed, none());
    expect_that!(stored.notification().output().times_sent, none());
    broker.subscriptions().delete(&subscription_id).send().await?;
    Ok(())
}
