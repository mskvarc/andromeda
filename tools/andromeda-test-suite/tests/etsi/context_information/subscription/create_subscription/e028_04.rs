//! A create-subscription request naming the id of an existing subscription is refused with 409 `AlreadyExists` (clause
//! 5.8.1).

use crate::{entity_operations::ld_json_body_client, subscription_payloads::building_subscription};
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
    model::{ErrorType, NgsiLdMediaType, Representation},
    subscription::{Endpoint, SubscriptionId},
};

/// Pins: Check that one cannot create a subscription with an existing id
/// Clause: 5.8.1
#[gtest]
#[tokio::test]
async fn e028_04_01_create_a_subscription_with_an_id_known_to_the_system() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Building"]).await?;
    let sink = NotificationSink::start().await?;
    let id: SubscriptionId = namespace.urn_text("Subscription", 1).parse()?;
    let endpoint = Endpoint::builder(sink.endpoint(1)?.as_str()).accept(NgsiLdMediaType::Json).build()?;
    let subscription = building_subscription(id.clone(), endpoint, Representation::Simplified)?;
    let client = ld_json_body_client(&broker, &vocabulary)?;
    let _subscription = Cleanup::of(&client).subscription_of(&subscription);
    client.subscriptions().create(&subscription).send().await?;

    let answer = client.with_responses().subscriptions().create(&subscription).send().await;
    verify_that!(&answer, problem(StatusCode::CONFLICT, ErrorType::AlreadyExists).with_title())?;

    broker.subscriptions().delete(&id).send().await.any_answer();
    Ok(())
}
