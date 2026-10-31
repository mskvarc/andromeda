//! A relayed null written as an array or a node reference stays the bare null.

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
    model::{APPLICATION_LD_JSON, NgsiLdMediaType, Representation},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: A relayed entity whose owner is `["urn:ngsi-ld:null"]` and whose driver is `{"@id": "urn:ngsi-ld:null"}`,
/// both expanding to the NGSI-LD Null, is notified under a normalized subscription with owner and driver the bare
/// urn:ngsi-ld:null, never a Property holding it
/// Clause: 5.8.6, 4.5.0
#[gtest]
#[tokio::test]
async fn n_ntf_pay_9239_a_relayed_null_written_as_an_array_or_a_node_reference_stays_the_bare_null() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let source = ContextSource::start().await?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let deletion_id = namespace.urn("Vehicle", "deletion")?;
    register_copy_source(&broker, &source, &namespace, "s1").await?;
    let subscription = Subscription::builder()
        .id(namespace.subscription_id(1)?)
        .entities([vehicle_type.as_str()])
        .notification(
            NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?)
                .format(Representation::Normalized)
                .build()?,
        )
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let copy_id = placed_copy_id(&source, "s1", &vehicle_type).await?;
    // The two spellings of the NGSI-LD Null are not attributes the entity model writes, so the data is written as text.
    let data = format!(
        r#"[{{"id":"{deletion_id}","type":"{vehicle_type}","owner":["urn:ngsi-ld:null"],"driver":{{"@id":"urn:ngsi-ld:null"}},"speed":{{"type":"Property","value":40}}}}]"#
    );
    let body = notification_text(&namespace, &copy_id, &data)?;
    let answer = relay(&broker, &APPLICATION_LD_JSON, &body).await?;
    expect_that!(&answer, answered(StatusCode::OK));
    let notifications = sink.wait_for_at_least(1, 1).await?;
    expect_that!(notifications, len(eq(1)));
    let received = notifications.first().or_fail()?;
    expect_that!(&received.body, json_equals("$.data[0].owner", "urn:ngsi-ld:null"));
    expect_that!(&received.body, json_equals("$.data[0].driver", "urn:ngsi-ld:null"));
    expect_that!(&received.body, json_equals("$.data[0].speed.value", 40));
    Ok(())
}
