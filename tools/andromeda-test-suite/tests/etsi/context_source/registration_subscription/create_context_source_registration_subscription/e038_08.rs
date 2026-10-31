//! A Context Source Registration Subscription that breaks the data types, cardinalities or restrictions of clause
//! 5.2.12 is refused with 400 `BadRequestData` (clause 5.11.2).

use crate::{
    entity_operations::ld_json_body_client,
    registration_subscription_operations::registration_subscription_request,
    subscription_payloads::{building_registration_subscription, key_values_document},
};
use andromeda_test_suite::{broker, cleanup::Cleanup, namespace::Namespace, notification_sink::NotificationSink, problem::problem, vocabulary::Vocabulary};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::UncheckedDocument,
    model::{ErrorType, RawJson},
    subscription::{Activation, SubscriptionId},
};

/// Pins: Check that one cannot create a context source registration subscription If the data types, cardinalities and
/// restrictions expressed by clause 5.2.12 are not met
/// Clause: 5.11.2
#[gtest]
#[tokio::test]
async fn e038_08_01_without_notification() -> Result<()> {
    refused(|document| Ok(document.remove_member("notification"))).await
}

/// Pins: Check that one cannot create a context source registration subscription If the data types, cardinalities and
/// restrictions expressed by clause 5.2.12 are not met
/// Clause: 5.11.2
#[gtest]
#[tokio::test]
async fn e038_08_02_invalid_type() -> Result<()> {
    refused(|document| Ok(document.set_text("type", "RandomType"))).await
}

/// Pins: Check that one cannot create a context source registration subscription If the data types, cardinalities and
/// restrictions expressed by clause 5.2.12 are not met
/// Clause: 5.11.2
#[gtest]
#[tokio::test]
async fn e038_08_03_invalid_query() -> Result<()> {
    refused(|document| Ok(document.set_text("q", "(invalidQuery"))).await
}

/// Pins: Check that one cannot create a context source registration subscription If the data types, cardinalities and
/// restrictions expressed by clause 5.2.12 are not met
/// Clause: 5.11.2
#[gtest]
#[tokio::test]
async fn e038_08_04_empty_watched_attributes() -> Result<()> {
    refused(|document| Ok(document.set_member("watchedAttributes", &"[]".parse::<RawJson>()?))).await
}

/// Creates the Building subscription `urn:ngsi-ld:Subscription:<ns>-1` with the vocabulary of the term `Building` as a
/// one-element `@context` list in the body, its document edited by `edit`, and requires 400 `BadRequestData` with a
/// title and a distinct detail.
async fn refused(edit: impl FnOnce(UncheckedDocument) -> Result<UncheckedDocument>) -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Building"]).await?;
    let sink = NotificationSink::start().await?;
    let id: SubscriptionId = namespace.urn_text("Subscription", 1).parse()?;
    let uri = sink.endpoint(1)?;
    let subscription = building_registration_subscription(Some(id), &uri, Activation::Unspecified, None)?;
    let document = edit(key_values_document(&subscription, &uri)?)?;
    let client = ld_json_body_client(&broker, &vocabulary)?;
    let _subscription = Cleanup::of(&client).registration_subscription_of(&subscription);
    let answer = client
        .send_unchecked(registration_subscription_request(&client, &subscription, &document)?)
        .await;
    verify_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_distinct_detail())
}
