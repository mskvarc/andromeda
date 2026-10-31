//! Update context source subscription with an unknown parameter is invalid request.

use andromeda_test_suite::{broker, namespace::Namespace, notification_sink::NotificationSink, problem::problem, wire::api_uri};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{headers::Accept, operation::UpdateSubscription},
    model::{ErrorType, NgsiLdMediaType},
    subscription::{Endpoint, NotificationParams, Subscription, SubscriptionFragment},
};

/// Pins: Update Context Source Registration Subscription with a URL parameter the operation does not define answers
/// `InvalidRequest` and changes nothing
/// Clause: 6.3.20, 5.11.3
/// Recorded: 6.3.20 says an incompatible parameter "should" give `InvalidRequest`
#[gtest]
#[tokio::test]
async fn o_cmn_err_2011_update_context_source_subscription_with_an_unknown_parameter_is_invalid_request() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let subscription_id = namespace.subscription_id(1)?;
    let device_type = namespace.entity_type("Device")?;
    let subscription = Subscription::registration_builder()
        .id(subscription_id.clone())
        .description("before")
        .entities([device_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint("s1")?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.registration_subscriptions().create(&subscription).send().await?;
    let fragment = SubscriptionFragment::registration_builder().description("after").build()?;
    // The operation defines no `invalidParam` parameter; the request carries it as written.
    let request = broker
        .unchecked(&UpdateSubscription {
            id: &subscription_id,
            fragment: &fragment,
        })?
        .uri(api_uri(&broker, &format!("csourceSubscriptions/{subscription_id}?invalidParam=invalidValue"))?);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registration_subscriptions()
        .retrieve(&subscription_id)
        .send()
        .await?;
    expect_that!(retrieved.description(), some(eq("before")));
    Ok(())
}
