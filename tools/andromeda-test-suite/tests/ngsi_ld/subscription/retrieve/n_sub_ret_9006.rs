//! Retrieve subscription refuses the listing parameters.

use andromeda_test_suite::{broker, namespace::Namespace, notification_sink::NotificationSink, problem::problem, wire::request_to};
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::ACCEPT};
use ngsi_ld::{
    http::{headers::Accept, operation::SystemAttributes},
    model::{ErrorType, NgsiLdMediaType},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: Retrieve Subscription with limit, offset or options=count answers 400 `InvalidRequest`, while
/// `options=sysAttrs` answers 200 with `createdAt`
/// Clause: 6.3.20, 6.3.11, Table 6.11.3.1-1, 5.8.3
#[gtest]
#[tokio::test]
async fn n_sub_ret_9006_retrieve_subscription_refuses_the_listing_parameters() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let subscription_id = namespace.subscription_id(1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([vehicle_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    // The client cannot write a parameter of this read, so it is sent as written.
    let request = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        &format!("subscriptions/urn:ngsi-ld:Subscription:{namespace}-1?limit=3"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest));
    // The client cannot write a parameter of this read, so it is sent as written.
    let request_2 = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        &format!("subscriptions/urn:ngsi-ld:Subscription:{namespace}-1?offset=1"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer_2 = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request_2).await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest));
    // The client cannot write a parameter of this read, so it is sent as written.
    let request_3 = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        &format!("subscriptions/urn:ngsi-ld:Subscription:{namespace}-1?options=count"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer_3 = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request_3).await;
    expect_that!(&answer_3, problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest));
    let subscription_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_id)
        .system_attributes(SystemAttributes::Include)
        .send()
        .await?;
    expect_that!(subscription_2.id(), eq(&subscription_id));
    expect_that!(subscription_2.output().created_at, some(anything()));
    broker.subscriptions().delete(&subscription_id).send().await?;
    Ok(())
}
