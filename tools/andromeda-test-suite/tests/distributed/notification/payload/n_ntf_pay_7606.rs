//! A change notification carries only the subscribed dataset instance.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::has_attribute,
    namespace::Namespace,
    notification_sink::NotificationSink,
    nth::nth,
    outcome::Done,
    problem::answered,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, operation::CreateSubscription},
    model::{Entity, Lossless, NgsiLdMediaType, Property, RawJson, Representation},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: Create Entity on broker A with two speed instances notifies a subscription created on broker A naming one
/// `datasetId` with that instance only
/// Clause: 5.8.6, 5.2.12, 4.5.5
#[gtest]
#[tokio::test]
async fn n_ntf_pay_7606_a_change_notification_carries_only_the_subscribed_dataset_instance() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let subscription_id = namespace.subscription_id(1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let one_dataset = namespace.dataset_id("Dataset", "one")?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([vehicle_type.as_str()])
        .notification(
            NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?)
                .format(Representation::Normalized)
                .build()?,
        )
        .build()?;
    // The model refuses it (a subscription datasetId), so `datasetId` is written as text.
    let dataset_id: RawJson = format!(r#"["urn:ngsi-ld:Dataset:{namespace}-one"]"#).parse()?;
    let document = UncheckedDocument::try_from(subscription.render())?.set_member("datasetId", &dataset_id);
    let request = broker.unchecked(&CreateSubscription { subscription: &subscription })?.document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    let vehicle = Entity::builder()
        .id(vehicle_id)
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(40).dataset_id(namespace.dataset_id("Dataset", "one")?))
        .property("speed", Property::builder(50).dataset_id(namespace.dataset_id("Dataset", "two")?))
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    sink.wait_for_at_least(1, 1).await?;
    let notifications = sink.received(1).await?;
    let notification = notifications.first().or_fail()?.entities(Lossless::Normalized)?;
    expect_that!(
        notification.data().as_slice(),
        nth(0, has_attribute("speed", any_attribute().with_dataset_id(one_dataset).value(40)))
    );
    Ok(())
}
