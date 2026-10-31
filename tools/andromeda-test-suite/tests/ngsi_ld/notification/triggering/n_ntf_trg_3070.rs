//! A subscription q trailing path into a json property named in json keys notifies the matching entity.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, notification_sink::NotificationSink, nth::nth, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, JsonProperty, Lossless, NgsiLdMediaType, RawJson, Representation},
    query::q::Query,
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: a subscription whose q has a trailing path into a `JsonProperty` named in its `jsonKeys` member is
/// evaluated against the raw member, and notifies the entity whose member equals the value and not one whose member
/// differs
/// Clause: 4.9, 5.2.12, 5.8.6
#[gtest]
#[tokio::test]
async fn n_ntf_trg_3070_a_subscription_q_trailing_path_into_a_json_property_named_in_json_keys_notifies_the_matching_entity() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let subscription_id = namespace.subscription_id(1)?;
    let car_type = namespace.entity_type("Car")?;
    let paid_id = namespace.urn("Car", "paid")?;
    let overstay_id = namespace.urn("Car", "overstay")?;
    let tickets = format!("tickets_{namespace}");
    let q = format!(r#"{tickets}[value]=="Overstay 60 minutes""#);
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([car_type.as_str()])
        .q(q.parse::<Query>()?)
        .json_keys([tickets.as_str()])
        .notification(
            NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?)
                .format(Representation::Normalized)
                .build()?,
        )
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_id)
        .send()
        .await?;
    expect_that!(retrieved.q().map(ToString::to_string), some(eq(q.as_str())));
    expect_that!(
        retrieved.json_keys().iter().map(ToString::to_string).collect::<Vec<_>>(),
        elements_are![eq(&tickets)]
    );
    // The entity whose member differs is created first: the oldest notification is the matching entity's.
    let paid = Entity::builder()
        .id(paid_id)
        .entity_type(car_type.clone())
        .json_property(tickets.as_str(), JsonProperty::builder(r#"{ "value": "Paid" }"#.parse::<RawJson>()?))
        .build()?;
    broker.entities().create(&paid).send().await?.done()?;
    let overstay = Entity::builder()
        .id(overstay_id.clone())
        .entity_type(car_type)
        .json_property(
            tickets.as_str(),
            JsonProperty::builder(r#"{ "value": "Overstay 60 minutes" }"#.parse::<RawJson>()?),
        )
        .build()?;
    broker.entities().create(&overstay).send().await?.done()?;
    let notifications = sink.wait_for_at_least(1, 1).await?;
    let notification = notifications.first().or_fail()?.entities(Lossless::Normalized)?;
    expect_that!(notification.data().as_slice(), nth(0, has_id(&overstay_id)));
    Ok(())
}
