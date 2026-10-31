//! An empty attributes pick or omit in a notification is bad request data and stores nothing.

use andromeda_test_suite::{broker, config::SuiteConfig, namespace::Namespace, problem::problem, subscription_document::create_request};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{ErrorType, Input, NgsiLdMediaType},
    query::projection,
    subscription::{Endpoint, NotificationParams, Subscription, SubscriptionId},
};

/// Pins: Create Subscription whose notification holds an empty attributes, pick or omit array answers 400
/// `BadRequestData` and stores nothing, while the same member holding one name creates the Subscription
/// Clause: 5.2.14, 5.5.4, 5.8.1.4
#[gtest]
#[tokio::test]
async fn n_sub_cre_9029_an_empty_attributes_pick_or_omit_in_a_notification_is_bad_request_data_and_stores_nothing() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let unreachable = config.unreachable_origin().origin().ascii_serialization();
    let building_type = namespace.entity_type("Building")?;
    let subscription = |id: &SubscriptionId, notification: NotificationParams<Input>| -> Result<Subscription<Input>> {
        Ok(Subscription::builder()
            .id(id.clone())
            .entities([building_type.as_str()])
            .notification(notification)
            .build()?)
    };
    let endpoint = || -> Result<Endpoint> { Ok(Endpoint::builder(format!("{unreachable}/notify")).build()?) };
    for (number, member) in [(1, "attributes"), (2, "pick"), (3, "omit")] {
        let id = namespace.subscription_id(number)?;
        // The model builds no empty attributes, pick or omit, so the member is added to the written body.
        let request = create_request(
            &broker,
            &subscription(&id, NotificationParams::builder(endpoint()?).build()?)?,
            &[(member, "[]")],
        )?;
        let refused = broker.send_unchecked(request).await;
        expect_that!(&refused, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
        let stored = broker
            .with_responses()
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .subscriptions()
            .retrieve(&id)
            .send()
            .await;
        expect_that!(&stored, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    }
    let attributes_id = namespace.subscription_id(4)?;
    let attributes = subscription(&attributes_id, NotificationParams::builder(endpoint()?).attributes(["name"]).build()?)?;
    broker.subscriptions().create(&attributes).send().await?;
    let stored = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&attributes_id)
        .send()
        .await?;
    expect_that!(stored.notification().attributes().first().map(ToString::to_string), some(eq("name")));
    broker.subscriptions().delete(&attributes_id).send().await?;
    let pick_id = namespace.subscription_id(5)?;
    let pick = projection::attributes(["name"]).build()?;
    let picked = subscription(&pick_id, NotificationParams::builder(endpoint()?).pick(pick.clone()).build()?)?;
    broker.subscriptions().create(&picked).send().await?;
    let stored_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&pick_id)
        .send()
        .await?;
    expect_that!(stored_2.notification().pick(), some(eq(&pick)));
    broker.subscriptions().delete(&pick_id).send().await?;
    let omit_id = namespace.subscription_id(6)?;
    let omit = projection::attributes(["name"]).build()?;
    let omitted = subscription(&omit_id, NotificationParams::builder(endpoint()?).omit(omit.clone()).build()?)?;
    broker.subscriptions().create(&omitted).send().await?;
    let stored_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&omit_id)
        .send()
        .await?;
    expect_that!(stored_3.notification().omit(), some(eq(&omit)));
    broker.subscriptions().delete(&omit_id).send().await?;
    Ok(())
}
