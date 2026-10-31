//! Create and retrieve context source subscription with an unknown parameter are invalid request.

use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    notification_sink::NotificationSink,
    problem::{answered, problem},
    wire::api_uri,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{
        headers::Accept,
        operation::{CreateSubscription, RetrieveSubscription},
    },
    model::{ErrorType, NgsiLdMediaType},
    subscription::{Endpoint, NotificationParams, RegistrationSubscription, Subscription},
};

/// Pins: Create and Retrieve Context Source Registration Subscription with a URL parameter the operation does not
/// define answer `InvalidRequest` and store nothing
/// Clause: 6.3.20, 5.11.2, 5.11.4
/// Recorded: 6.3.20 says an incompatible parameter "should" give `InvalidRequest`
#[gtest]
#[tokio::test]
async fn o_cmn_err_2008_create_and_retrieve_context_source_subscription_with_an_unknown_parameter_are_invalid_request() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let subscription_2_id = namespace.subscription_id(2)?;
    let subscription_id = namespace.subscription_id(1)?;
    let device_type = namespace.entity_type("Device")?;
    let subscription_2 = Subscription::registration_builder()
        .id(subscription_2_id.clone())
        .entities([device_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint("s2")?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    // The operation defines no `invalidParam` parameter; the request carries it as written.
    let request = broker
        .unchecked(&CreateSubscription { subscription: &subscription_2 })?
        .uri(api_uri(&broker, "csourceSubscriptions?invalidParam=invalidValue")?);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest));
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registration_subscriptions()
        .retrieve(&subscription_2_id)
        .send()
        .await;
    expect_that!(&answer_2, answered(StatusCode::NOT_FOUND));
    let subscription = Subscription::registration_builder()
        .id(subscription_id.clone())
        .entities([device_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint("s1")?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.registration_subscriptions().create(&subscription).send().await?;
    // The operation defines no `invalidParam` parameter; the request carries it as written.
    let request_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .unchecked(&RetrieveSubscription::builder().id(&subscription_id).kind(RegistrationSubscription).build())?
        .uri(api_uri(&broker, &format!("csourceSubscriptions/{subscription_id}?invalidParam=invalidValue"))?);
    let answer_3 = broker.send_unchecked(request_2).await;
    expect_that!(&answer_3, problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest));
    Ok(())
}
