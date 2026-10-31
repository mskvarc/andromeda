//! Refuses an empty notification trigger and keeps the stored one.

use andromeda_test_suite::{broker, namespace::Namespace, notification_sink::NotificationSink, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::UpdateSubscription},
    model::{ErrorType, NgsiLdMediaType, RawJson},
    subscription::{Endpoint, NotificationParams, NotificationTrigger, NotificationTriggers, Subscription, SubscriptionFragment},
};

/// Pins: a fragment setting `notificationTrigger` to an empty array is refused with `BadRequestData` and the stored
/// triggers stay as they were
/// Clause: 5.2.12, 5.8.2.4
/// Recorded: Table 5.2.12-1 lists the valid trigger values and gives `notificationTrigger` cardinality 0..1, but
/// unlike entities and `watchedAttributes` it does not say that an empty array is not allowed; Andromeda reads an
/// empty list as naming no trigger, a subscription that could never fire, and refuses it
#[gtest]
#[tokio::test]
async fn o_sub_upd_0004_refuses_an_empty_notification_trigger_and_keeps_the_stored_one() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let subscription_id = namespace.subscription_id(1)?;
    let building_type = namespace.entity_type("Building")?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([building_type.as_str()])
        .watched_attributes(["name"])
        .notification_triggers([NotificationTrigger::EntityCreated, NotificationTrigger::EntityUpdated])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let fragment = SubscriptionFragment::builder().build()?;
    // The model refuses it (the document breaks the rules of its data type at notificationTrigger), so `notificationTrigger` is written as text.
    let notification_trigger: RawJson = "[]".parse()?;
    let document = UncheckedDocument::try_from(fragment.render())?.set_member("notificationTrigger", &notification_trigger);
    let request = broker
        .unchecked(&UpdateSubscription {
            id: &subscription_id,
            fragment: &fragment,
        })?
        .document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let subscription_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_id)
        .send()
        .await?;
    expect_that!(subscription_2.notification_triggers().map(|triggers| triggers.iter().count()), some(eq(2)));
    expect_that!(
        subscription_2
            .notification_triggers()
            .into_iter()
            .flat_map(NotificationTriggers::iter)
            .collect::<Vec<_>>(),
        contains(eq(&NotificationTrigger::EntityCreated))
    );
    expect_that!(
        subscription_2
            .notification_triggers()
            .into_iter()
            .flat_map(NotificationTriggers::iter)
            .collect::<Vec<_>>(),
        contains(eq(&NotificationTrigger::EntityUpdated))
    );
    Ok(())
}
