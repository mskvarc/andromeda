//! An update whose `expiresAt` refers to a `DateTime` in the past is refused with 400 `BadRequestData` (clause 5.8.2).

use crate::{entity_operations::ld_json_body_client, subscription_operations::create_timed_building_subscription};
use andromeda_test_suite::{broker, namespace::Namespace, notification_sink::NotificationSink, problem::problem, teardown::AnyAnswer, vocabulary::Vocabulary};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    model::{DateTime, ErrorType},
    subscription::{SubscriptionFragment, SubscriptionId},
};

/// Pins: Check that one cannot update a subscription: If expiresAt is included but referring to a `DateTime` in the
/// past, then a `BadRequestData` error shall be raised
/// Clause: 5.8.2
#[gtest]
#[tokio::test]
async fn e029_11_01_update_subscription_with_expires_at_in_the_past() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Building"]).await?;
    let sink = NotificationSink::start().await?;
    let id: SubscriptionId = namespace.urn_text("Subscription", 1).parse()?;
    let client = ld_json_body_client(&broker, &vocabulary)?;
    let _subscription = create_timed_building_subscription(&client, &id, &sink.endpoint(1)?).await?;

    let fragment = SubscriptionFragment::builder()
        .expires_at("2020-08-01T22:07:00Z".parse::<DateTime>()?)
        .build()?;
    let answer = broker.with_responses().subscriptions().update(&id, &fragment).send().await;
    verify_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title())?;

    broker.subscriptions().delete(&id).send().await.any_answer();
    Ok(())
}
