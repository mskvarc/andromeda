//! Update a csource subscription without a content type is 415.

use andromeda_test_suite::{
    broker,
    core_context::core_context_list,
    namespace::Namespace,
    notification_sink::NotificationSink,
    problem::answered,
    wire::request_to,
};
use bytes::Bytes;
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::ContextDelivery,
    model::NgsiLdMediaType,
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: Update Context Source Registration Subscription with a body and no Content-Type header answers 415
/// Clause: 6.3.4, 5.11.3
#[gtest]
#[tokio::test]
async fn n_css_upd_9001_update_a_csource_subscription_without_a_content_type_is_415() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let core_context_list_body = broker.with_context(core_context_list()?)?.with_context_delivery(ContextDelivery::LdJsonBody);
    let subscription_id = namespace.subscription_id(1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let subscription = Subscription::registration_builder()
        .id(subscription_id.clone())
        .entities([vehicle_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    core_context_list_body.registration_subscriptions().create(&subscription).send().await?;
    // The body goes without a Content-Type, which no typed operation sends, so it is sent as written.
    let request = request_to(&broker, Method::PATCH, &format!("csourceSubscriptions/urn:ngsi-ld:Subscription:{namespace}-1"))?
        .body(Bytes::from("{\"description\": \"no content type\"}"));
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::UNSUPPORTED_MEDIA_TYPE));
    broker.registration_subscriptions().delete(&subscription_id).send().await?;
    Ok(())
}
