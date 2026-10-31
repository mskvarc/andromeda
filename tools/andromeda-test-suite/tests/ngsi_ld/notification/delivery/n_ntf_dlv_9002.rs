//! A receiver info authorization reaches the endpoint as a header.

use andromeda_test_suite::{broker, header_condition::HeaderCondition, namespace::Namespace, notification_sink::NotificationSink, outcome::Done};
use googletest::prelude::*;
use http::HeaderName;
use ngsi_ld::{
    model::{Entity, NgsiLdMediaType, Value},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: A notification of a Subscription whose endpoint `receiverInfo` holds the key Authorization reaches the
/// endpoint with an Authorization header equal to its value
/// Clause: 5.2.22, 6.3.8
#[gtest]
#[tokio::test]
async fn n_ntf_dlv_9002_a_receiver_info_authorization_reaches_the_endpoint_as_a_header() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let subscription_receiver_info_id = namespace.subscription_id("receiver-info")?;
    let building_type = namespace.entity_type("Building")?;
    let building_receiver_info_id = namespace.urn("Building", "receiver-info")?;
    let subscription_receiver_info = Subscription::builder()
        .id(subscription_receiver_info_id.clone())
        .entities([building_type.as_str()])
        .notification(
            NotificationParams::builder(
                Endpoint::builder(sink.endpoint("receiver-info")?)
                    .accept(NgsiLdMediaType::Json)
                    .receiver_info("Authorization", format!("Bearer secret-token-{namespace}"))
                    .build()?,
            )
            .build()?,
        )
        .build()?;
    broker.subscriptions().create(&subscription_receiver_info).send().await?;
    let building_receiver_info = Entity::builder()
        .id(building_receiver_info_id)
        .entity_type(building_type.clone())
        .property("temperature", Value::Number("21.5".parse()?))
        .build()?;
    broker.entities().create(&building_receiver_info).send().await?.done()?;
    let fields = [HeaderCondition::Is(
        HeaderName::from_static("authorization"),
        format!("Bearer secret-token-{namespace}"),
    )];
    sink.wait_for_at_least_matching("receiver-info", &[], &fields, 1).await?;
    broker.subscriptions().delete(&subscription_receiver_info_id).send().await?;
    Ok(())
}
