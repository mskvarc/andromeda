//! A copy is placed at the sources whose registration shares a `datasetId` with the subscription or lists none.

use andromeda_test_suite::{
    broker,
    cleanup::Cleanup,
    context_source::ContextSource,
    dataset_copy_source::register_dataset_copy_source,
    forwarded_body,
    namespace::Namespace,
    notification_sink::NotificationSink,
    relayed_copy::placed_copy,
};
use googletest::prelude::*;
use http::Method;
use ngsi_ld::{
    model::{DatasetId, Input, NgsiLdMediaType},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// The copy of a subscription the source, beneath its root path `prefix`, received last.
async fn latest_copy(source: &ContextSource, prefix: &str) -> Result<Subscription<Input>> {
    let copies = source.received_on(Method::POST, &format!("/{prefix}/ngsi-ld/v1/subscriptions")).await?;
    Ok(forwarded_body::subscription(copies.last().or_fail()?)?)
}

/// Pins: Create Subscription by type, over four `federationOps` registrations of the type listing dataset a, dataset
/// b, none, and `@none` with a, places with `datasetId` a and b a copy at each source, carrying a, b, a and b, and a;
/// places with `datasetId` c a copy at the registration listing none alone, carrying c; and places with no
/// `datasetId` a copy at each source, carrying the registration's list, none for the registration listing none
/// Clause: 5.12, 4.3.6.1, 5.2.9, 5.2.12, 5.8.1.4
#[gtest]
#[tokio::test]
async fn n_sub_cre_6817_a_copy_is_placed_at_the_sources_whose_registration_shares_a_dataset_id_or_lists_none() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let source = ContextSource::start().await?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let dataset_a = namespace.dataset_id("Dataset", "a")?;
    let dataset_b = namespace.dataset_id("Dataset", "b")?;
    let dataset_c = namespace.dataset_id("Dataset", "c")?;
    let no_dataset = "@none".parse::<DatasetId>()?;
    register_dataset_copy_source(&broker, &source, &namespace, "s1", 1, Some(vec![dataset_a.clone()])).await?;
    register_dataset_copy_source(&broker, &source, &namespace, "s2", 2, Some(vec![dataset_b.clone()])).await?;
    register_dataset_copy_source(&broker, &source, &namespace, "s3", 3, None).await?;
    register_dataset_copy_source(&broker, &source, &namespace, "s4", 4, Some(vec![no_dataset.clone(), dataset_a.clone()])).await?;
    let _cleanup = Cleanup::of(&broker)
        .registration(&namespace.registration_id(1)?)
        .registration(&namespace.registration_id(2)?)
        .registration(&namespace.registration_id(3)?)
        .registration(&namespace.registration_id(4)?)
        .subscription(&namespace.subscription_id(1)?)
        .subscription(&namespace.subscription_id(2)?)
        .subscription(&namespace.subscription_id(3)?);
    let by_a_and_b = Subscription::builder()
        .id(namespace.subscription_id(1)?)
        .entities([vehicle_type.as_str()])
        .dataset_ids([dataset_a.clone(), dataset_b.clone()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&by_a_and_b).send().await?;
    let copy_s1 = forwarded_body::subscription(&placed_copy(&source, "s1", &vehicle_type).await?)?;
    let copy_s2 = forwarded_body::subscription(&placed_copy(&source, "s2", &vehicle_type).await?)?;
    let copy_s3 = forwarded_body::subscription(&placed_copy(&source, "s3", &vehicle_type).await?)?;
    let copy_s4 = forwarded_body::subscription(&placed_copy(&source, "s4", &vehicle_type).await?)?;
    expect_that!(copy_s1.dataset_ids(), some(elements_are![eq(&dataset_a)]));
    expect_that!(copy_s2.dataset_ids(), some(elements_are![eq(&dataset_b)]));
    expect_that!(copy_s3.dataset_ids(), some(elements_are![eq(&dataset_a), eq(&dataset_b)]));
    expect_that!(copy_s4.dataset_ids(), some(elements_are![eq(&dataset_a)]));
    let by_c = Subscription::builder()
        .id(namespace.subscription_id(2)?)
        .entities([vehicle_type.as_str()])
        .dataset_ids([dataset_c.clone()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(2)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&by_c).send().await?;
    let copies_s3 = source.wait_for_on(Method::POST, "/s3/ngsi-ld/v1/subscriptions", 2).await?;
    expect_that!(copies_s3, len(eq(2)));
    expect_that!(latest_copy(&source, "s3").await?.dataset_ids(), some(elements_are![eq(&dataset_c)]));
    // Copies are placed before Create Subscription answers, so a copy for another source would have arrived by now.
    for prefix in ["s1", "s2", "s4"] {
        expect_that!(
            source.received_on(Method::POST, &format!("/{prefix}/ngsi-ld/v1/subscriptions")).await?,
            len(eq(1))
        );
    }
    let unnamed = Subscription::builder()
        .id(namespace.subscription_id(3)?)
        .entities([vehicle_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(3)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&unnamed).send().await?;
    for prefix in ["s1", "s2", "s4"] {
        expect_that!(
            source.wait_for_on(Method::POST, &format!("/{prefix}/ngsi-ld/v1/subscriptions"), 2).await?,
            len(eq(2))
        );
    }
    expect_that!(source.wait_for_on(Method::POST, "/s3/ngsi-ld/v1/subscriptions", 3).await?, len(eq(3)));
    expect_that!(latest_copy(&source, "s1").await?.dataset_ids(), some(elements_are![eq(&dataset_a)]));
    expect_that!(latest_copy(&source, "s2").await?.dataset_ids(), some(elements_are![eq(&dataset_b)]));
    expect_that!(latest_copy(&source, "s3").await?.dataset_ids(), none());
    expect_that!(
        latest_copy(&source, "s4").await?.dataset_ids(),
        some(elements_are![eq(&no_dataset), eq(&dataset_a)])
    );
    Ok(())
}
