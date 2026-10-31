//! A temporal q on an entity subscription is bad request data.

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

/// Pins: Create Subscription carrying a well-formed `temporalQ` between with an `endTimeAt` answers 400
/// `BadRequestData` naming `temporalQ` and stores nothing
/// Clause: Table 5.2.12-1, 5.8.1.4
#[gtest]
#[tokio::test]
async fn n_sub_cre_9019_a_temporal_q_on_an_entity_subscription_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let unreachable = config.unreachable_origin().origin().ascii_serialization();
    let subscription_id = namespace.subscription_id(1)?;
    let temporal_probe_type = namespace.entity_type("TemporalProbe")?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([temporal_probe_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(format!("{unreachable}/notify")).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    // The model writes no `temporalQ` on an entity subscription, so the member is added to the written document.
    let temporal_q: RawJson = r#"{"timerel": "between", "timeAt": "2020-01-01T00:00:00Z", "endTimeAt": "2020-02-01T00:00:00Z"}"#.parse()?;
    let document = UncheckedDocument::try_from(subscription.render())?.set_member("temporalQ", &temporal_q);
    let request = broker.unchecked(&CreateSubscription { subscription: &subscription })?.document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(
        &answer,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_body_containing("temporalQ")
    );
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
