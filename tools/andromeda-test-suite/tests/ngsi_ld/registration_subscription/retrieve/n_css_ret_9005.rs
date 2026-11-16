//! A csource subscription reads back the `datasetId` it was written with.

use andromeda_test_suite::{broker, cleanup::Cleanup, namespace::Namespace, notification_sink::NotificationSink};
use googletest::prelude::*;
use ngsi_ld::{
    model::NgsiLdMediaType,
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: Create Context Source Registration Subscription with `datasetId` of one dataset answers 201, and Retrieve
/// answers that `datasetId` as written
/// Clause: 5.11.2, 5.11.4.5, 5.2.12
#[gtest]
#[tokio::test]
async fn n_css_ret_9005_a_csource_subscription_reads_back_the_dataset_id_it_was_written_with() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let subscription_id = namespace.subscription_id(1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let dataset_a = namespace.dataset_id("Dataset", "a")?;
    let subscription = Subscription::registration_builder()
        .id(subscription_id.clone())
        .entities([vehicle_type.as_str()])
        .dataset_ids([dataset_a.clone()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    let _cleanup = Cleanup::of(&broker).registration_subscription(&subscription_id);
    broker.registration_subscriptions().create(&subscription).send().await?;
    let retrieved = broker.registration_subscriptions().retrieve(&subscription_id).send().await?;
    expect_that!(retrieved.dataset_ids(), some(elements_are![eq(&dataset_a)]));
    Ok(())
}
