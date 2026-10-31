//! A periodic csource subscription with throttling is bad request data.

use andromeda_test_suite::{broker, namespace::Namespace, notification_sink::NotificationSink, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, operation::CreateSubscription},
    model::{ErrorType, NgsiLdMediaType, RawJson},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: Create Context Source Registration Subscription with `timeInterval` 600 and throttling 5 answers 400
/// `BadRequestData`
/// Clause: Table 5.2.12-1, 5.11.2.4
#[gtest]
#[tokio::test]
async fn n_css_cre_9004_a_periodic_csource_subscription_with_throttling_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let subscription_id = namespace.subscription_id(1)?;
    let building_type = namespace.entity_type("Building")?;
    let subscription = Subscription::registration_builder()
        .id(subscription_id.clone())
        .entities([building_type.as_str()])
        .throttling(5)
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    // The model refuses it (the document breaks the rules of its data type), so `timeInterval` is written as text.
    let time_interval: RawJson = "600".parse()?;
    let document = UncheckedDocument::try_from(subscription.render())?.set_member("timeInterval", &time_interval);
    let request = broker.unchecked(&CreateSubscription { subscription: &subscription })?.document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    Ok(())
}
