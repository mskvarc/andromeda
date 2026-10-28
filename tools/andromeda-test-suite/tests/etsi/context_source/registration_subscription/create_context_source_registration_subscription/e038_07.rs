//! ETSI `038_07`: a Context Source Registration Subscription whose id another one holds is refused with 409
//! `AlreadyExists` (clause 5.11.2).

use crate::{
    suite_keywords::{create_registration_subscription, ld_json_body_client, registration_subscription_request},
    suite_payloads::{building_registration_subscription, key_values_document},
};
use andromeda_test_suite::{
    broker,
    cleanup::Cleanup,
    namespace::Namespace,
    notification_sink::NotificationSink,
    problem::problem,
    teardown::AnyAnswer,
    vocabulary::Vocabulary,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    model::ErrorType,
    subscription::{Activation, SubscriptionId},
};

/// Pins: Check that one cannot create a context source registration subscription where another context source
/// registration subscription whose id is equivalent exists
/// Clause: 5.11.2
#[gtest]
#[tokio::test]
async fn e038_07_01_create_existing_context_source_registration_subscription() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Building"]).await?;
    let sink = NotificationSink::start().await?;
    let id: SubscriptionId = namespace.urn_text("Subscription", 1).parse()?;
    let uri = sink.endpoint(1)?;
    let subscription = building_registration_subscription(Some(id.clone()), &uri, Activation::Unspecified, None)?;
    let client = ld_json_body_client(&broker, &vocabulary)?;
    let _subscription = create_registration_subscription(&client, &subscription, &uri).await?;

    let _subscription = Cleanup::of(&client).registration_subscription_of(&subscription);
    let request = registration_subscription_request(&client, &subscription, &key_values_document(&subscription, &uri)?)?;
    let answer = client.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::CONFLICT, ErrorType::AlreadyExists).with_distinct_detail());

    broker.registration_subscriptions().delete(&id).send().await.any_answer();
    Ok(())
}
