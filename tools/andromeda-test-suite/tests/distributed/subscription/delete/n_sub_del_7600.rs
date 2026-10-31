//! A deletion removes it and bad or unknown ids are refused.

use andromeda_test_suite::{broker, namespace::Namespace, notification_sink::NotificationSink, problem::problem, wire::request_to};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::headers::Accept,
    model::{ErrorType, NgsiLdMediaType, Representation},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: Delete Subscription on broker A answers 204 and broker A then answers 404; an id that is not a URI answers
/// 400 `BadRequestData` and an unknown id 404 `ResourceNotFound`
/// Clause: 5.8.5.4
#[gtest]
#[tokio::test]
async fn n_sub_del_7600_a_deletion_removes_it_and_bad_or_unknown_ids_are_refused() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let subscription_id = namespace.subscription_id(1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([vehicle_type.as_str()])
        .notification(
            NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?)
                .format(Representation::Normalized)
                .build()?,
        )
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    broker.subscriptions().delete(&subscription_id).send().await?;
    let answer = broker
        .with_responses()
        .with_accept(Accept::any())
        .subscriptions()
        .retrieve(&subscription_id)
        .send()
        .await;
    expect_that!(&answer, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    // The model refuses the id this request names, so it is sent as written.
    let request = request_to(&broker, Method::DELETE, "subscriptions/not-a-uri")?;
    let answer_2 = broker.send_unchecked(request).await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_3 = broker.with_responses().subscriptions().delete(&subscription_id).send().await;
    expect_that!(&answer_3, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    Ok(())
}
