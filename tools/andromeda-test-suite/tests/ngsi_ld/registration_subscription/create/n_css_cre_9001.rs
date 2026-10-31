//! A csource subscription without a notification trigger reads back without one.

use andromeda_test_suite::{broker, core_context::core_context_list, namespace::Namespace, notification_sink::NotificationSink};
use googletest::prelude::*;
use ngsi_ld::{
    http::{ContextDelivery, headers::Accept},
    model::NgsiLdMediaType,
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: Create Context Source Registration Subscription without `notificationTrigger` answers 201, and Retrieve
/// answers it without `notificationTrigger`
/// Clause: 5.11.2.4, 5.11.4.5, Table 5.2.12-1
#[gtest]
#[tokio::test]
async fn n_css_cre_9001_a_csource_subscription_without_a_notification_trigger_reads_back_without_one() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let core_context_list_body = broker.with_context(core_context_list()?)?.with_context_delivery(ContextDelivery::LdJsonBody);
    let subscription_id = namespace.subscription_id(1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let subscription = Subscription::registration_builder()
        .id(subscription_id.clone())
        .entities([vehicle_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    core_context_list_body.registration_subscriptions().create(&subscription).send().await?;
    let subscription_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registration_subscriptions()
        .retrieve(&subscription_id)
        .send()
        .await?;
    expect_that!(subscription_2.notification_triggers(), none());
    broker.registration_subscriptions().delete(&subscription_id).send().await?;
    Ok(())
}
