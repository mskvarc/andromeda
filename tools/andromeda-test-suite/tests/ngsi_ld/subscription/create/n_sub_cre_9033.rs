//! A Subscription entity selector with an unknown member is bad request data.

use andromeda_test_suite::{
    broker,
    config::SuiteConfig,
    namespace::Namespace,
    problem::{answered, problem},
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::CreateSubscription},
    model::{ErrorType, NgsiLdMediaType, RawJson},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: Create Subscription whose `entities` selector names a Vehicle type and the member `ids`, which Table 5.2.33-1
/// does not list, answers 400 `BadRequestData` and stores nothing
/// Clause: 5.2.33, 5.5.4, 5.8.1.4
#[gtest]
#[tokio::test]
async fn n_sub_cre_9033_a_subscription_entity_selector_with_an_unknown_member_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let unreachable = config.unreachable_origin().origin().ascii_serialization();
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let subscription_id = namespace.subscription_id("unknown-member")?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([vehicle_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(format!("{unreachable}/notify")).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    // The model writes no selector member beyond the table's, so `entities` is written as text.
    let entities: RawJson = format!(r#"[{{"type": "{vehicle_type}", "ids": ["{}"]}}]"#, namespace.urn("Vehicle", 1)?).parse()?;
    let document = UncheckedDocument::try_from(subscription.render())?.set_member("entities", &entities);
    let request = broker.unchecked(&CreateSubscription { subscription: &subscription })?.document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let stored = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_id)
        .send()
        .await;
    expect_that!(&stored, answered(StatusCode::NOT_FOUND));
    Ok(())
}
