//! An unreachable jsonld context is ld context not available.

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
    http::headers::Accept,
    model::{ErrorType, NgsiLdMediaType, Representation},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: Create Subscription on broker A whose `jsonldContext` names an origin where nothing listens answers 504
/// `LdContextNotAvailable` and stores nothing
/// Clause: 5.8.1.4, 5.2.12, 6.3.2
#[gtest]
#[tokio::test]
async fn n_sub_cre_7603_an_unreachable_jsonld_context_is_ld_context_not_available() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let config = SuiteConfig::get()?;
    let unreachable = config.unreachable_origin().origin().ascii_serialization();
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
        .jsonld_context(format!("{unreachable}/context.jsonld"))
        .build()?;
    let answer = broker.with_responses().subscriptions().create(&subscription).send().await;
    expect_that!(&answer, problem(StatusCode::GATEWAY_TIMEOUT, ErrorType::LdContextNotAvailable));
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::any())
        .subscriptions()
        .retrieve(&subscription_id)
        .send()
        .await;
    expect_that!(&answer_2, answered(StatusCode::NOT_FOUND));
    Ok(())
}
