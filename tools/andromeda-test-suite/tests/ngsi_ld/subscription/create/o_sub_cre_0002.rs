//! Refuses an empty notification trigger and stores nothing.

use andromeda_test_suite::{broker, namespace::Namespace, notification_sink::NotificationSink, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::CreateSubscription},
    model::{ErrorType, NgsiLdMediaType, RawJson},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: a subscription whose `notificationTrigger` is an empty array is refused with `BadRequestData` and is not
/// stored
/// Clause: 5.2.12, 5.8.1.4
/// Recorded: Table 5.2.12-1 lists the valid trigger values and gives `notificationTrigger` cardinality 0..1, but
/// unlike entities and `watchedAttributes` it does not say that an empty array is not allowed; Andromeda reads an
/// empty list as naming no trigger, a subscription that could never fire, and refuses it
#[gtest]
#[tokio::test]
async fn o_sub_cre_0002_refuses_an_empty_notification_trigger_and_stores_nothing() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let subscription_id = namespace.subscription_id(1)?;
    let building_type = namespace.entity_type("Building")?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([building_type.as_str()])
        .watched_attributes(["name"])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    // The model refuses it (the document breaks the rules of its data type at notificationTrigger), so `notificationTrigger` is written as text.
    let notification_trigger: RawJson = "[]".parse()?;
    let document = UncheckedDocument::try_from(subscription.render())?.set_member("notificationTrigger", &notification_trigger);
    let request = broker.unchecked(&CreateSubscription { subscription: &subscription })?.document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_id)
        .send()
        .await;
    expect_that!(&answer_2, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    Ok(())
}
