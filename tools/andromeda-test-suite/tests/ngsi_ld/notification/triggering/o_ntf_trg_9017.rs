//! Json keys decides whether a subscription is notified.

use andromeda_test_suite::{
    attribute_matcher::{any_attribute, attribute_of},
    broker,
    entity_matcher::has_attribute,
    namespace::Namespace,
    notification_sink::NotificationSink,
    nth::nth,
    outcome::Done,
    problem::answered,
    value::object,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, operation::CreateSubscription},
    model::{AttributeKind, Entity, EntityFragment, JsonProperty, Lossless, NgsiLdMediaType, RawJson, Value},
    query::{EntitySelector, builders::SelectorTypeBuilder, q},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: Of two Subscriptions with q address`[city]`=="Berlin" over an Entity whose address Property holds {"city":
/// "Berlin"}, only the one without `jsonKeys` is notified of a change; once address is a `JsonProperty` holding the
/// same object, the one with `jsonKeys` address is
/// Clause: 4.9, Table 5.2.12-1, 5.8.6
/// Recorded: Table 5.2.12-1 has `jsonKeys` keep the named attributes from expansion, and 4.9 shows it on a
/// `JsonProperty` only; Andromeda leaves the trailing path unexpanded, which meets the raw keys of a `JsonProperty`
/// and not the expanded keys of a Property value
#[gtest]
#[tokio::test]
async fn o_ntf_trg_9017_json_keys_decides_whether_a_subscription_is_notified() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let building_keys_id = namespace.urn("Building", "keys")?;
    let building_type = namespace.entity_type("Building")?;
    let subscription_expanded_id = namespace.subscription_id("expanded")?;
    let subscription_keyed_id = namespace.subscription_id("keyed")?;
    let building_keys = Entity::builder()
        .id(building_keys_id.clone())
        .entity_type(building_type.clone())
        .property("address", object([("city", Value::from("Berlin"))]))
        .build()?;
    broker.entities().create(&building_keys).send().await?.done()?;
    let subscription_expanded = Subscription::builder()
        .id(subscription_expanded_id.clone())
        .entities([EntitySelector::new(SelectorTypeBuilder::from(&building_type).build()?).with_ids(building_keys_id.clone())])
        .q(q::attr("address").member("city").eq("Berlin").build()?)
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint("expanded")?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&subscription_expanded).send().await?;
    let subscription_keyed = Subscription::builder()
        .id(subscription_keyed_id.clone())
        .entities([EntitySelector::new(SelectorTypeBuilder::from(&building_type).build()?).with_ids(building_keys_id.clone())])
        .q(q::attr("address").member("city").eq("Berlin").build()?)
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint("keyed")?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    // The model refuses it (a subscription expandValues or jsonKeys), so `jsonKeys` is written as text.
    let json_keys: RawJson = r#""address""#.parse()?;
    let document = UncheckedDocument::try_from(subscription_keyed.render())?.set_member("jsonKeys", &json_keys);
    let request = broker
        .unchecked(&CreateSubscription {
            subscription: &subscription_keyed,
        })?
        .document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    let fragment = EntityFragment::builder().property("note", "json-keys").build()?;
    broker.entities().append_attributes(&building_keys_id, &fragment).send().await?.done()?;
    sink.wait_for_at_least("expanded", 1).await?;
    let notifications = sink.received("expanded").await?;
    let notification = notifications.first().or_fail()?.entities(Lossless::Normalized)?;
    expect_that!(
        notification.data().as_slice(),
        nth(0, has_attribute("note", any_attribute().value("json-keys")))
    );
    // The keyed Subscription is owed a notification of the later change to a JsonProperty, and notifications of one subscription are delivered in order, so a notification of the first change would be in its journal before it.
    let fragment_2 = EntityFragment::builder()
        .json_property("address", JsonProperty::builder(r#"{"city":"Berlin"}"#.parse::<RawJson>()?))
        .build()?;
    broker.entities().update_attributes(&building_keys_id, &fragment_2).send().await?.done()?;
    sink.wait_for_at_least("keyed", 1).await?;
    let notifications_2 = sink.received("keyed").await?;
    expect_that!(notifications_2, len(eq(1)));
    let notification_2 = notifications_2.first().or_fail()?.entities(Lossless::Normalized)?;
    expect_that!(
        notification_2.data().as_slice(),
        nth(0, has_attribute("address", attribute_of(AttributeKind::JsonProperty)))
    );
    broker.subscriptions().delete(&subscription_expanded_id).send().await?;
    broker.subscriptions().delete(&subscription_keyed_id).send().await?;
    Ok(())
}
