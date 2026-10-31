//! A Context Source Registration Subscription is retrieved with the members it was created with, and the `Link` field
//! of the answer names the context it was asked in (clause 5.11.4).

use crate::{
    entity_operations::{context_link, ld_json_body_client},
    registration_subscription_operations::{
        REGISTRATION_SUBSCRIPTION_STATE,
        create_registration_subscription,
        expect_absent_or_date,
        expect_absent_or_integer,
        without_members,
    },
    subscription_payloads::building_registration_subscription,
};
use andromeda_test_suite::{
    broker,
    comparison::matches_document,
    namespace::Namespace,
    notification_sink::NotificationSink,
    problem::answered,
    teardown::AnyAnswer,
    vocabulary::Vocabulary,
    wire::{header_value, json_body},
};
use googletest::prelude::*;
use http::{StatusCode, header::LINK};
use ngsi_ld::{
    model::NgsiLdMediaType,
    subscription::{Activation, SubscriptionId},
};
use serde_json::json;

/// Pins: Check that one can retrieve a context source registration subscription
/// Clause: 5.11.4
/// Expected: a retrieved subscription is compared with jsonldContext left out (clause 5.8.1.4)
#[gtest]
#[tokio::test]
async fn e040_01_01_retrieve_context_source_registration_subscription() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Building"]).await?;
    let sink = NotificationSink::start().await?;
    let id: SubscriptionId = namespace.urn_text("Subscription", 1).parse()?;
    let uri = sink.endpoint(1)?;
    let subscription = building_registration_subscription(Some(id.clone()), &uri, Activation::Unspecified, None)?;
    let _subscription = create_registration_subscription(&ld_json_body_client(&broker, &vocabulary)?, &subscription, &uri).await?;

    let reader = broker.with_context(vocabulary.context())?.with_accept(NgsiLdMediaType::Json);
    let received = reader.with_responses().registration_subscriptions().retrieve(&id).send().await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let received = received?;
    let link = context_link(&vocabulary)?;
    expect_that!(header_value(received.head.headers(), &LINK), some(eq(link.to_str()?)));
    let body = json_body(&received)?;
    expect_absent_or_date(&body, "lastNotification");
    expect_absent_or_date(&body, "lastFailure");
    expect_absent_or_date(&body, "lastSuccess");
    expect_absent_or_integer(&body, "timesSent");
    expect_that!(
        &without_members(without_members(body, &REGISTRATION_SUBSCRIPTION_STATE), &["isActive"]),
        matches_document(json!({
            "id": id.to_string(),
            "type": "Subscription",
            "entities": [{"type": "Building"}],
            "notification": {
                "format": "keyValues",
                "endpoint": {"uri": uri.as_str(), "accept": "application/json"}
            },
        }))
    );

    broker.registration_subscriptions().delete(&id).send().await.any_answer();
    Ok(())
}
