//! A relayed deleted attribute stays the bare null in the simplified format.

use andromeda_test_suite::{
    body_json::json_equals,
    broker,
    context_source::ContextSource,
    namespace::Namespace,
    notification_sink::NotificationSink,
    problem::answered,
    relayed_copy::{notification_text, placed_copy_id, register_copy_source, relay},
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    model::{APPLICATION_LD_JSON, NgsiLdMediaType, Representation, SimplifiedSpelling},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: A relayed entity whose owner is urn:ngsi-ld:null is notified under a `keyValues` subscription with owner
/// the bare urn:ngsi-ld:null
/// Clause: 5.8.6
#[gtest]
#[tokio::test]
async fn n_ntf_pay_6812_a_relayed_deleted_attribute_stays_the_bare_null_in_the_simplified_format() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let source = ContextSource::start().await?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    register_copy_source(&broker, &source, &namespace, "s1").await?;
    let subscription = Subscription::builder()
        .id(namespace.subscription_id(1)?)
        .entities([vehicle_type.as_str()])
        .notification(
            NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?)
                .format(Representation::Simplified)
                .simplified_spelling(SimplifiedSpelling::KeyValues)
                .build()?,
        )
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let copy_id = placed_copy_id(&source, "s1", &vehicle_type).await?;
    // The entity model writes no NGSI-LD Null as the bare value of an attribute, so the data is written as text.
    let data = format!(
        r#"[{{"id":"{}","type":"{vehicle_type}","owner":"urn:ngsi-ld:null","speed":{{"type":"Property","value":40}}}}]"#,
        namespace.urn_text("Vehicle", "deletion")
    );
    let body = notification_text(&namespace, &copy_id, &data)?;
    let answer = relay(&broker, &APPLICATION_LD_JSON, &body).await?;
    expect_that!(&answer, answered(StatusCode::OK));
    let notifications = sink.wait_for_at_least(1, 1).await?;
    expect_that!(notifications, len(eq(1)));
    // A bare NGSI-LD Null as an attribute is no value the entity model reads, so the notification is read as received.
    expect_that!(&notifications.first().or_fail()?.body, json_equals("$.data[0].owner", "urn:ngsi-ld:null"));
    Ok(())
}
