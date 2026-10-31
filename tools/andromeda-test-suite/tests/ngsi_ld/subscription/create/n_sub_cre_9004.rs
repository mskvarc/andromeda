//! An entity subscription without a notification trigger reads back the default.

use andromeda_test_suite::{broker, core_context::core_context_list, namespace::Namespace, notification_sink::NotificationSink};
use googletest::prelude::*;
use ngsi_ld::{
    http::{ContextDelivery, headers::Accept},
    model::NgsiLdMediaType,
    subscription::{Endpoint, NotificationParams, NotificationTrigger, NotificationTriggers, Subscription},
};

/// Pins: Create Subscription without `notificationTrigger` answers 201, and Retrieve Subscription answers
/// `notificationTrigger` as `attributeCreated` and `attributeUpdated`
/// Clause: Table 5.2.12-1, 5.8.1.4, 5.8.3.4
#[gtest]
#[tokio::test]
async fn n_sub_cre_9004_an_entity_subscription_without_a_notification_trigger_reads_back_the_default() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let core_context_list_body = broker.with_context(core_context_list()?)?.with_context_delivery(ContextDelivery::LdJsonBody);
    let subscription_id = namespace.subscription_id(1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([vehicle_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    core_context_list_body.subscriptions().create(&subscription).send().await?;
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
        contains(eq(&NotificationTrigger::AttributeCreated))
    );
    expect_that!(
        subscription_2
            .notification_triggers()
            .into_iter()
            .flat_map(NotificationTriggers::iter)
            .collect::<Vec<_>>(),
        contains(eq(&NotificationTrigger::AttributeUpdated))
    );
    broker.subscriptions().delete(&subscription_id).send().await?;
    Ok(())
}
