//! A csource subscription without type is bad request data while a fragment without it updates.

use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    notification_sink::NotificationSink,
    problem::{answered, problem},
    wire::request_to,
};
use bytes::Bytes;
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::CONTENT_TYPE};
use ngsi_ld::{
    http::headers::Accept,
    model::{ErrorType, NgsiLdMediaType},
    subscription::{Endpoint, NotificationParams, Subscription, SubscriptionFragment},
};

/// Pins: Create Context Source Registration Subscription without type answers 400 `BadRequestData` and stores
/// nothing, while Update Context Source Registration Subscription with a fragment without type answers 204 and
/// applies it
/// Clause: 5.11.2.4, 5.11.3.4, 5.8.1.4, Table 5.2.12-1
#[gtest]
#[tokio::test]
async fn n_css_cre_9009_a_csource_subscription_without_type_is_bad_request_data_while_a_fragment_without_it_updates() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let subscription_id = namespace.subscription_id(1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let endpoint = sink.endpoint(1)?;
    // The model refuses this subscription, so it is sent as written.
    let request = request_to(&broker, Method::POST, "csourceSubscriptions")?.append_header(CONTENT_TYPE, HeaderValue::from_static("application/json")).body(Bytes::from(format!("{{\n    \"id\": \"urn:ngsi-ld:Subscription:{namespace}-1\",\n    \"entities\": [\n        {{\n            \"type\": \"Vehicle_{namespace}\"\n        }}\n    ],\n    \"notification\": {{\n        \"endpoint\": {{\n            \"uri\": \"{endpoint}\",\n            \"accept\": \"application/json\"\n        }}\n    }}\n}}")));
    let answer = broker.send_unchecked(request).await;
    expect_that!(
        &answer,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_detail_containing("type")
    );
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registration_subscriptions()
        .retrieve(&subscription_id)
        .send()
        .await;
    expect_that!(&answer_2, answered(StatusCode::NOT_FOUND));
    let subscription = Subscription::registration_builder()
        .id(subscription_id.clone())
        .entities([vehicle_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.registration_subscriptions().create(&subscription).send().await?;
    let fragment = SubscriptionFragment::registration_builder().subscription_name("renamed").build()?;
    broker.registration_subscriptions().update(&subscription_id, &fragment).send().await?;
    let subscription_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registration_subscriptions()
        .retrieve(&subscription_id)
        .send()
        .await?;
    expect_that!(subscription_2.name(), some(eq("renamed")));
    broker.registration_subscriptions().delete(&subscription_id).send().await?;
    Ok(())
}
