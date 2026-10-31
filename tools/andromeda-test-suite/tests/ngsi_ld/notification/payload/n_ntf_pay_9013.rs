//! A notification keeps a one element json array and its previous json.

use andromeda_test_suite::{
    attribute_matcher::attribute_of,
    broker,
    entity_matcher::has_attribute,
    namespace::Namespace,
    notification_sink::NotificationSink,
    nth::nth,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    model::{Attribute, AttributeKind, AttributeName, Entity, JsonProperty, Lossless, NgsiLdMediaType, RawJson, Representation},
    subscription::{ChangeReporting, Endpoint, NotificationParams, NotificationTrigger, Subscription},
};
use serde_json::json;

/// Pins: A `showChanges` notification of a Partial Attribute Update replacing a `JsonProperty` holding a
/// one-element array carries the new one-element array as json and the replaced one as `previousJson`
/// Clause: 5.8.6, 4.5.24.2
#[gtest]
#[tokio::test]
async fn n_ntf_pay_9013_a_notification_keeps_a_one_element_json_array_and_its_previous_json() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let subscription_id = namespace.subscription_id("json-array")?;
    let notified_type = namespace.entity_type("JsonArrayNotified")?;
    let notified_id = namespace.urn("JsonArrayNotified", 1)?;
    let notified = Entity::builder()
        .id(notified_id.clone())
        .entity_type(notified_type.clone())
        .json_property("tickets", JsonProperty::builder(r#"[{"code": "a"}]"#.parse::<RawJson>()?))
        .build()?;
    broker.entities().create(&notified).send().await?.done()?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([notified_type.as_str()])
        .notification_triggers([NotificationTrigger::AttributeUpdated])
        .notification(
            NotificationParams::builder(Endpoint::builder(sink.endpoint("json")?).accept(NgsiLdMediaType::Json).build()?)
                .format(Representation::Normalized)
                .show_changes(ChangeReporting::WithPrevious)
                .build()?,
        )
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let patch = Attribute::from(JsonProperty::builder(r#"[{"code": "b"}]"#.parse::<RawJson>()?).build()?).into_patch();
    broker
        .entities()
        .update_attribute(&notified_id, &"tickets".parse::<AttributeName>()?, patch)
        .send()
        .await?
        .done()?;
    let notifications = sink.wait_for_at_least("json", 1).await?;
    expect_that!(notifications, len(eq(1)));
    let notification = notifications.first().or_fail()?.entities(Lossless::Normalized)?;
    expect_that!(
        notification.data().as_slice(),
        nth(
            0,
            has_attribute(
                "tickets",
                attribute_of(AttributeKind::JsonProperty)
                    .json(json!([{"code": "b"}]))
                    .previous_json(json!([{"code": "a"}]))
            )
        )
    );
    broker.subscriptions().delete(&subscription_id).send().await?;
    Ok(())
}
