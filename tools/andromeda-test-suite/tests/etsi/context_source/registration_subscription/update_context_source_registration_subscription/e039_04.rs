//! An update whose fragment breaks the data types or restrictions of clause 5.2.12 is refused with 400 `BadRequestData`
//! (clause 5.11.3).

use crate::{
    entity_operations::ld_json_body_client,
    registration_subscription_operations::{create_registration_subscription, update_registration_subscription_request},
    subscription_payloads::{building_registration_subscription, key_values_notification},
};
use andromeda_test_suite::{broker, namespace::Namespace, notification_sink::NotificationSink, problem::problem, teardown::AnyAnswer, vocabulary::Vocabulary};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::UncheckedDocument,
    model::{ErrorType, RawJson},
    subscription::{Activation, SubscriptionId},
};

/// Pins: Check that one cannot update a context source registration subscription with a fragment that does not meet
/// the data types and restrictions expressed by clause 5.2.12
/// Clause: 5.11.3
#[gtest]
#[tokio::test]
async fn e039_04_01_invalid_type() -> Result<()> {
    refused(|uri| {
        Ok(UncheckedDocument::empty()
            .null_member("type")
            .set_member("notification", &key_values_notification(uri)?))
    })
    .await
}

/// Pins: Check that one cannot update a context source registration subscription with a fragment that does not meet
/// the data types and restrictions expressed by clause 5.2.12
/// Clause: 5.11.3
#[gtest]
#[tokio::test]
async fn e039_04_02_invalid_notification() -> Result<()> {
    refused(|_| Ok(UncheckedDocument::empty().set_member("notification", &r#"{"format":"keyValues"}"#.parse::<RawJson>()?))).await
}

/// Creates the Building subscription `urn:ngsi-ld:Subscription:<ns>-1` notifying endpoint 1, updates it in the
/// vocabulary's `Link` context with the body `fragment` writes for endpoint 2, requires 400 `BadRequestData` with a
/// title and a distinct detail, and deletes the subscription.
async fn refused(fragment: impl FnOnce(&url::Url) -> Result<UncheckedDocument>) -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Building"]).await?;
    let sink = NotificationSink::start().await?;
    let id: SubscriptionId = namespace.urn_text("Subscription", 1).parse()?;
    let uri = sink.endpoint(1)?;
    let subscription = building_registration_subscription(Some(id.clone()), &uri, Activation::Unspecified, None)?;
    let _subscription = create_registration_subscription(&ld_json_body_client(&broker, &vocabulary)?, &subscription, &uri).await?;

    let linked = broker.with_context(vocabulary.context())?;
    let document = fragment(&sink.endpoint(2)?)?;
    let answer = linked.send_unchecked(update_registration_subscription_request(&linked, &id, &document)?).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_distinct_detail());

    broker.registration_subscriptions().delete(&id).send().await.any_answer();
    Ok(())
}
