//! ETSI `047_16`: an update of the `entities` of a Context Source Registration Subscription notifies every
//! registration that matches it now (clause 5.11.7).

use crate::{
    context_source::registration_subscription::context_source_registration_subscription_notification_behaviour::registration_notification_case::registered,
    suite_keywords::{create_registration_subscription, csource_endpoint, ld_json_body_client},
    suite_payloads::building_registration_subscription,
};
use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    ngsild_date::is_ngsild_date,
    notification_sink::NotificationSink,
    robot_literal,
    teardown::AnyAnswer,
    vocabulary::Vocabulary,
};
use googletest::prelude::*;
use ngsi_ld::{
    query::EntitySelector,
    registration::{CSourceRegistration, RegistrationId, RegistrationInfo},
    subscription::{Activation, SubscriptionFragment, SubscriptionId},
};
use serde_json::{Value, json};

/// Pins: Check if one updates a context source registration subscription, a `CsourceNotification` will be sent with
/// all currently matching context source registrations
/// Clause: 5.11.7
/// Changed: Wait for notification parses the body as json.loads('''<body>'''), a Python '''...''' literal, so a
/// backslash in the body is read as an escape and a ''' or a final ' ends the literal; the port refuses all three (not
/// contains "\\", not contains "'''", not endsWith "'") and then requires the body to be JSON, which is stricter only
/// where an escape leaves the keyword reading, and accepting, a body other than the one sent
#[gtest]
#[tokio::test]
async fn e047_16_01_match_first_context_source_registration() -> Result<()> {
    matched(&["Vehicle"], |body, [first, _]| {
        expect_that!(body.pointer("/data/0/id"), some(eq(&json!(first.to_string()))));
        expect_that!(body.pointer("/data").and_then(Value::as_array).map(Vec::len), some(eq(1)));
    })
    .await
}

/// Pins: Check if one updates a context source registration subscription, a `CsourceNotification` will be sent with
/// all currently matching context source registrations
/// Clause: 5.11.7
/// Changed: Wait for notification parses the body as json.loads('''<body>'''), a Python '''...''' literal, so a
/// backslash in the body is read as an escape and a ''' or a final ' ends the literal; the port refuses all three (not
/// contains "\\", not contains "'''", not endsWith "'") and then requires the body to be JSON, which is stricter only
/// where an escape leaves the keyword reading, and accepting, a body other than the one sent
#[gtest]
#[tokio::test]
async fn e047_16_02_match_second_context_source_registration() -> Result<()> {
    matched(&["Bus"], |body, [_, second]| {
        expect_that!(body.pointer("/data/0/id"), some(eq(&json!(second.to_string()))));
        expect_that!(body.pointer("/data").and_then(Value::as_array).map(Vec::len), some(eq(1)));
    })
    .await
}

/// Pins: Check if one updates a context source registration subscription, a `CsourceNotification` will be sent with
/// all currently matching context source registrations
/// Clause: 5.11.7
/// Changed: Wait for notification parses the body as json.loads('''<body>'''), a Python '''...''' literal, so a
/// backslash in the body is read as an escape and a ''' or a final ' ends the literal; the port refuses all three (not
/// contains "\\", not contains "'''", not endsWith "'") and then requires the body to be JSON, which is stricter only
/// where an escape leaves the keyword reading, and accepting, a body other than the one sent
#[gtest]
#[tokio::test]
async fn e047_16_03_match_both_context_source_registrations() -> Result<()> {
    matched(&["Vehicle", "Bus"], |body, [first, second]| {
        let [first, second] = [json!(first.to_string()), json!(second.to_string())];
        expect_that!(body.pointer("/data/0/id"), some(any![eq(&first), eq(&second)]));
        expect_that!(body.pointer("/data/1/id"), some(any![eq(&first), eq(&second)]));
        expect_that!(body.pointer("/data").and_then(Value::as_array).map(Vec::len), some(eq(2)));
    })
    .await
}

/// Subscribes to every Building (201), registers the Vehicle source `-1` and the Bus source `-2` (201 each), updates
/// the subscription's `entities` to `types` in the vocabulary's `Link` context, requires the first notification within
/// 5 s to be the newly matching `ContextSourceNotification` of the subscription notified at a date whose `data`
/// `check` accepts, and deletes the subscription whatever the answer and both registrations, requiring a status
/// below 400.
async fn matched(types: &[&str], check: impl FnOnce(&Value, [&RegistrationId; 2])) -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Building", "Vehicle", "Bus"]).await?;
    let sink = NotificationSink::start().await?;
    let id: SubscriptionId = namespace.urn_text("Subscription", 1).parse()?;
    let uri = sink.endpoint(1)?;
    let subscription = building_registration_subscription(Some(id.clone()), &uri, Activation::Unspecified, None)?;
    let _subscription = create_registration_subscription(&ld_json_body_client(&broker, &vocabulary)?, &subscription, &uri).await?;
    let endpoint = csource_endpoint(&namespace, "my.csource.org:1026").await?;
    let first: RegistrationId = namespace.urn_text("ContextSourceRegistration", 1).parse()?;
    let second: RegistrationId = namespace.urn_text("ContextSourceRegistration", 2).parse()?;
    let mut registrations = Vec::new();
    for (registration_id, entity_type) in [(&first, "Vehicle"), (&second, "Bus")] {
        let registration = CSourceRegistration::builder(endpoint.as_str())
            .id(registration_id.clone())
            .information([RegistrationInfo::builder().entities([entity_type]).build()?])
            .build()?;
        registrations.push(registered(&broker, &vocabulary, &registration).await?);
    }
    let _registrations = registrations;

    let selectors = types
        .iter()
        .map(|entity_type| Ok(EntitySelector::new(entity_type.parse()?)))
        .collect::<Result<Vec<_>>>()?;
    let change = SubscriptionFragment::registration_builder().entities(selectors).build()?;
    broker
        .with_context(vocabulary.context())?
        .registration_subscriptions()
        .update(&id, &change)
        .send()
        .await
        .any_answer();

    let notification = sink.wait_for(1, 1).await?;
    let body = robot_literal::read(&notification.body)?;
    expect_that!(body.pointer("/type"), some(eq(&json!("ContextSourceNotification"))));
    expect_that!(body.pointer("/subscriptionId"), some(eq(&json!(id.to_string()))));
    expect_that!(body.pointer("/notifiedAt").and_then(Value::as_str).map(is_ngsild_date), some(eq(true)));
    check(&body, [&first, &second]);
    expect_that!(body.pointer("/triggerReason"), some(eq(&json!("newlyMatching"))));

    broker.registration_subscriptions().delete(&id).send().await.any_answer();
    broker.registrations().delete(&first).send().await?;
    broker.registrations().delete(&second).send().await?;
    Ok(())
}
