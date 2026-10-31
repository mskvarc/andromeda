//! A resolvable jsonld context is stored and a non string one is bad request data.

use andromeda_test_suite::{
    broker,
    config::SuiteConfig,
    namespace::Namespace,
    notification_sink::NotificationSink,
    problem::{answered, problem},
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::CreateSubscription},
    model::{ErrorType, NgsiLdMediaType, RawJson},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: Create Subscription naming the core context as `jsonldContext` reads back with that `jsonldContext`, and
/// one whose `jsonldContext` is the number 42 answers 400 `BadRequestData` and stores nothing
/// Clause: 5.8.1.4, 5.2.12
#[gtest]
#[tokio::test]
async fn n_sub_cre_6804_a_resolvable_jsonld_context_is_stored_and_a_non_string_one_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let config = SuiteConfig::get()?;
    let subscription_id = namespace.subscription_id(1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let subscription_2_id = namespace.subscription_id(2)?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([vehicle_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .jsonld_context(config.core_context().clone())
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let subscription_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_id)
        .send()
        .await?;
    expect_that!(subscription_2.jsonld_context(), some(eq(config.core_context())));
    let subscription_2_2 = Subscription::builder()
        .id(subscription_2_id.clone())
        .entities([vehicle_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(2)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    // The model refuses it (`jsonldContext` must be a string at jsonldContext), so `jsonldContext` is written as text.
    let jsonld_context: RawJson = "42".parse()?;
    let document = UncheckedDocument::try_from(subscription_2_2.render())?.set_member("jsonldContext", &jsonld_context);
    let request = broker
        .unchecked(&CreateSubscription {
            subscription: &subscription_2_2,
        })?
        .document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_2_id)
        .send()
        .await;
    expect_that!(&answer_2, answered(StatusCode::NOT_FOUND));
    Ok(())
}
