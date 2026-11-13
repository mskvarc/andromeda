//! A notification carrying a custom receiver info header is a well formed message with one content type.

use andromeda_test_suite::{broker, namespace::Namespace, notification_sink::NotificationSink, outcome::Done};
use googletest::prelude::*;
use http::header::{AUTHORIZATION, CONTENT_LENGTH, CONTENT_TYPE, TRANSFER_ENCODING};
use ngsi_ld::{
    model::{Entity, NgsiLdMediaType, Value},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: A notification of a Subscription whose http endpoint asks for `application/json` and whose `receiverInfo`
/// holds the key Authorization reaches the endpoint with exactly one `Content-Type`, `application/json`, exactly one
/// `Content-Length`, equal to the length of its body, no `Transfer-Encoding`, and the Authorization header
/// Clause: 5.2.15, 6.3.8
/// Recorded: one `Content-Type` and a length that matches the body are what IETF RFC 9110 sections 5.3 and 8.6 make of
/// a well-formed request; the clause adds the custom header beside them
#[gtest]
#[tokio::test]
async fn n_ntf_dlv_9012_a_notification_carrying_a_custom_receiver_info_header_is_a_well_formed_message_with_one_content_type() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let subscription_id = namespace.subscription_id("headers")?;
    let building_type = namespace.entity_type("Building")?;
    let token = format!("Bearer token-{namespace}");
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([building_type.as_str()])
        .notification(
            NotificationParams::builder(
                Endpoint::builder(sink.endpoint("headers")?)
                    .accept(NgsiLdMediaType::Json)
                    .receiver_info("Authorization", token.as_str())
                    .build()?,
            )
            .build()?,
        )
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let building = Entity::builder()
        .id(namespace.urn("Building", "headers")?)
        .entity_type(building_type.clone())
        .property("temperature", Value::Number("21.5".parse()?))
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    let received = sink.wait_for("headers", 1).await?;
    let body_length = received.body.len().to_string();
    expect_that!(
        received
            .headers
            .get_all(CONTENT_TYPE)
            .iter()
            .map(|value| value.to_str().ok())
            .collect::<Vec<_>>(),
        elements_are![some(eq(&"application/json"))]
    );
    expect_that!(
        received
            .headers
            .get_all(CONTENT_LENGTH)
            .iter()
            .map(|value| value.to_str().ok())
            .collect::<Vec<_>>(),
        elements_are![some(eq(&body_length.as_str()))]
    );
    expect_that!(received.headers.get(TRANSFER_ENCODING), none());
    expect_that!(
        received
            .headers
            .get_all(AUTHORIZATION)
            .iter()
            .map(|value| value.to_str().ok())
            .collect::<Vec<_>>(),
        elements_are![some(eq(&token.as_str()))]
    );
    broker.subscriptions().delete(&subscription_id).send().await?;
    Ok(())
}
