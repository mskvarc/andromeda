//! A minimal Context Source Registration Subscription is created and retrieved with the members it was created with
//! (clause 5.11.2).

use crate::{
    entity_operations::ld_json_body_client,
    registration_subscription_operations::{
        REGISTRATION_SUBSCRIPTION_STATE,
        expect_absent_or_date,
        expect_absent_or_integer,
        expect_registration_subscription_created,
        registration_subscription_json,
        without_members,
    },
    subscription_payloads::building_registration_subscription,
};
use andromeda_test_suite::{
    broker,
    comparison::matches_document,
    namespace::Namespace,
    notification_sink::NotificationSink,
    teardown::AnyAnswer,
    vocabulary::Vocabulary,
};
use googletest::prelude::*;
use ngsi_ld::{
    model::NgsiLdMediaType,
    subscription::{Activation, SubscriptionId},
};
use serde_json::json;

/// Pins: Check that one can create a minimal context source registration subscription
/// Clause: 5.11.2
/// Expected: the created or updated registration subscription is compared with jsonldContext left out (clause 5.8.1.4)
#[gtest]
#[tokio::test]
async fn e038_01_01_create_context_source_registration_subscription() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Building"]).await?;
    let sink = NotificationSink::start().await?;
    let id: SubscriptionId = namespace.urn_text("Subscription", 1).parse()?;
    let uri = sink.endpoint(1)?;
    let subscription = building_registration_subscription(Some(id.clone()), &uri, Activation::Unspecified, None)?;
    let _subscription = expect_registration_subscription_created(&ld_json_body_client(&broker, &vocabulary)?, &subscription, &uri).await?;

    let reader = broker.with_context(vocabulary.context())?.with_accept(NgsiLdMediaType::JsonLd);
    let body = registration_subscription_json(&reader, &id).await?;
    expect_absent_or_date(&body, "lastNotification");
    expect_absent_or_date(&body, "lastFailure");
    expect_absent_or_date(&body, "lastSuccess");
    expect_absent_or_integer(&body, "timesSent");
    expect_that!(
        &without_members(body, &REGISTRATION_SUBSCRIPTION_STATE),
        matches_document(json!({
            "id": id.to_string(),
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
