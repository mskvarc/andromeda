//! An entity without a type is left out of the relayed notification.

use andromeda_test_suite::{
    broker,
    context_source::ContextSource,
    entity_lookup::with_id,
    namespace::Namespace,
    notification_sink::NotificationSink,
    problem::answered,
    relayed_copy::{notification_text, placed_copy_id, register_copy_source, relay},
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    model::{APPLICATION_LD_JSON, Lossless, NgsiLdMediaType},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: A Notification of the copy whose data holds two valid entities and one without a type answers 200 and the
/// subscriber is sent the two valid entities
/// Clause: 5.8.6
/// Recorded: 5.8.6 does not say how a receiver treats an entity of a Notification that cannot be read, and
/// Andromeda relays the others
#[gtest]
#[tokio::test]
async fn o_ntf_pay_6806_an_entity_without_a_type_is_left_out_of_the_relayed_notification() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let source = ContextSource::start().await?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let first_id = namespace.urn("Vehicle", "invalid-1")?;
    let second_id = namespace.urn("Vehicle", "invalid-2")?;
    let third_id = namespace.urn("Vehicle", "invalid-3")?;
    register_copy_source(&broker, &source, &namespace, "s1").await?;
    let subscription = Subscription::builder()
        .id(namespace.subscription_id(1)?)
        .entities([vehicle_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let copy_id = placed_copy_id(&source, "s1", &vehicle_type).await?;
    // An entity without `type` is no entity the model writes, so the data is written as text.
    let data = format!(
        r#"[{{"id":"{first_id}","type":"{vehicle_type}","speed":{{"type":"Property","value":40}}}},{{"id":"{second_id}","speed":{{"type":"Property","value":41}}}},{{"id":"{third_id}","type":"{vehicle_type}","speed":{{"type":"Property","value":42}}}}]"#
    );
    let body = notification_text(&namespace, &copy_id, &data)?;
    let answer = relay(&broker, &APPLICATION_LD_JSON, &body).await?;
    expect_that!(&answer, answered(StatusCode::OK));
    let notifications = sink.wait_for_at_least(1, 1).await?;
    expect_that!(notifications, len(eq(1)));
    let notification = notifications.first().or_fail()?.entities(Lossless::Normalized)?;
    expect_that!(notification.data(), len(eq(2)));
    expect_that!(with_id(notification.data(), &first_id), some(anything()));
    expect_that!(with_id(notification.data(), &third_id), some(anything()));
    expect_that!(with_id(notification.data(), &second_id), none());
    Ok(())
}
