//! ETSI `038_01`: a minimal Context Source Registration Subscription is created and retrieved with the members it was
//! created with (clause 5.11.2).

use crate::{
    suite_keywords::{
        REGISTRATION_SUBSCRIPTION_STATE,
        expect_absent_or_date,
        expect_absent_or_integer,
        expect_registration_subscription_created,
        ld_json_body_client,
        registration_subscription_json,
        without_members,
    },
    suite_payloads::building_registration_subscription,
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
/// Corrected: fix(csrsub-jsonldcontext): compare csr-subscription create/update with jsonldcontext ignored (5.8.1.4)
/// Changed: Check Response Headers Containing URI set to checks Location only when the response header dictionary holds
/// a key spelled exactly `Location`, which a broker sending a lower-case `location` (valid per RFC 9110, and what
/// Andromeda sends) never satisfies; Hurl matches header names without regard to case and cannot make an assertion
/// conditional, so the port requires a Location header that contains the subscription id
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
