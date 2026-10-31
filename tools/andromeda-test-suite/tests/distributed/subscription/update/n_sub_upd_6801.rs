//! An unreachable context is ld context not available and a non context is bad request data.

use andromeda_test_suite::{
    broker,
    config::SuiteConfig,
    namespace::Namespace,
    notification_sink::NotificationSink,
    nth::nth,
    problem::problem,
    wire::request_to,
};
use bytes::Bytes;
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::CONTENT_TYPE};
use iri_rs::IriRefBuf;
use ngsi_ld::{
    http::{ContextDelivery, headers::Accept},
    model::{AttributeName, Context, ContextEntry, ErrorType, NgsiLdMediaType},
    subscription::{Endpoint, NotificationParams, Subscription, SubscriptionFragment},
};

/// Pins: Update Subscription with an @context at an unreachable origin answers 504 `LdContextNotAvailable`, one
/// whose @context is the number 42 answers 400 `BadRequestData`, and the stored `watchedAttributes` stay as they
/// were
/// Clause: 5.8.2.4, 6.3.2
#[gtest]
#[tokio::test]
async fn n_sub_upd_6801_an_unreachable_context_is_ld_context_not_available_and_a_non_context_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let config = SuiteConfig::get()?;
    let unreachable = config.unreachable_origin().origin().ascii_serialization();
    let subscription_id = namespace.subscription_id(1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let body_context = broker
        .with_context(Context::Single(ContextEntry::Reference(IriRefBuf::new(format!(
            "{unreachable}/context.jsonld"
        ))?)))?
        .with_context_delivery(ContextDelivery::LdJsonBody);
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([vehicle_type.as_str()])
        .watched_attributes(["speed"])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let fragment = SubscriptionFragment::builder()
        .watched_attributes([namespace.attribute("speed")?.to_string()])
        .build()?;
    let answer = body_context.with_responses().subscriptions().update(&subscription_id, &fragment).send().await;
    expect_that!(&answer, problem(StatusCode::GATEWAY_TIMEOUT, ErrorType::LdContextNotAvailable));
    // The body goes as application/ld+json, which no typed operation sends, so it is sent as written.
    let request = request_to(&broker, Method::PATCH, &format!("subscriptions/urn:ngsi-ld:Subscription:{namespace}-1"))?
        .append_header(CONTENT_TYPE, HeaderValue::from_static("application/ld+json"))
        .body(Bytes::from(format!(
            "{{\n    \"watchedAttributes\": [\n        \"speed_{namespace}\"\n    ],\n    \"@context\": 42\n}}"
        )));
    let answer_2 = broker.send_unchecked(request).await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let subscription_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_id)
        .send()
        .await?;
    expect_that!(subscription_2.watched_attributes(), len(eq(1)));
    expect_that!(subscription_2.watched_attributes(), nth(0, eq(&"speed".parse::<AttributeName>()?)));
    Ok(())
}
