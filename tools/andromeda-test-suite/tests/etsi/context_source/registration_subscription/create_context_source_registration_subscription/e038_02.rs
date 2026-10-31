//! A Context Source Registration Subscription created without an id is given one, named by the `Location` of the answer
//! (clause 5.11.2).

use crate::{
    entity_operations::ld_json_body_client,
    registration_subscription_operations::{
        REGISTRATION_SUBSCRIPTION_STATE,
        registration_subscription_json,
        registration_subscription_request,
        without_members,
    },
    subscription_payloads::{building_registration_subscription, key_values_document},
};
use andromeda_test_suite::{
    broker,
    cleanup::Cleanup,
    comparison::matches_document,
    namespace::Namespace,
    notification_sink::NotificationSink,
    problem::answered,
    teardown::AnyAnswer,
    vocabulary::Vocabulary,
    wire::header_value,
};
use googletest::prelude::*;
use http::{StatusCode, header::LOCATION};
use ngsi_ld::{
    model::NgsiLdMediaType,
    subscription::{Activation, SubscriptionId},
};
use serde_json::json;

/// The members of the retrieved `notification` the case leaves uncompared.
const NOTIFICATION_STATE: [&str; 6] = ["lastNotification", "lastSuccess", "lastFailure", "timesSent", "timesFailed", "status"];

/// Pins: Check that one can create a context source registration subscription without providing an id and it will be
/// automatically generated
/// Clause: 5.11.2
/// Expected: the created or updated registration subscription is compared with jsonldContext left out (clause 5.8.1.4)
#[gtest]
#[tokio::test]
async fn e038_02_01_create_context_source_registration_subscription_without_an_id() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Building"]).await?;
    let sink = NotificationSink::start().await?;
    let uri = sink.endpoint(1)?;
    let subscription = building_registration_subscription(None, &uri, Activation::Unspecified, None)?;
    let client = ld_json_body_client(&broker, &vocabulary)?;
    let _subscription = Cleanup::of(&client).registration_subscription_of(&subscription);
    let request = registration_subscription_request(&client, &subscription, &key_values_document(&subscription, &uri)?)?;
    let answer = client.send_unchecked(request).await;
    verify_that!(&answer, answered(StatusCode::CREATED))?;
    let answer = answer?;
    let location = header_value(answer.headers(), &LOCATION).or_fail()?;
    expect_that!(answer.body().len(), eq(0));
    let id: SubscriptionId = location.rsplit('/').next().or_fail()?.parse()?;

    let reader = broker.with_context(vocabulary.context())?.with_accept(NgsiLdMediaType::JsonLd);
    let mut body = without_members(registration_subscription_json(&reader, &id).await?, &REGISTRATION_SUBSCRIPTION_STATE);
    if let Some(notification) = body.get_mut("notification") {
        *notification = without_members(notification.take(), &NOTIFICATION_STATE);
    }
    expect_that!(
        &without_members(body, &["id"]),
        matches_document(json!({
            "type": "Subscription",
            "entities": [{"type": "Building"}],
            "notification": {
                "format": "keyValues",
                "endpoint": {"uri": uri.as_str(), "accept": "application/json"}
            },
            "@context": vocabulary.url().as_str(),
        }))
    );

    broker.registration_subscriptions().delete(&id).send().await.any_answer();
    Ok(())
}
