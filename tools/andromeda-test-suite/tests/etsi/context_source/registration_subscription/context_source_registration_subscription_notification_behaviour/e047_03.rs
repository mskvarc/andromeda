//! A csource notification carries only the `RegistrationInfo` elements of the registration that match the subscription
//! (clause 5.11.7).

use crate::{
    context_source::registration_subscription::context_source_registration_subscription_notification_behaviour::registration_notification_case::{
        expect_csource_notification,
        registered_any,
        teardown,
    },
    entity_operations::ld_json_body_client,
    registration_operations::csource_endpoint,
    registration_subscription_operations::create_registration_subscription,
    subscription_payloads::building_registration_subscription,
};
use andromeda_test_suite::{broker, namespace::Namespace, notification_sink::NotificationSink, vocabulary::Vocabulary};
use googletest::prelude::*;
use ngsi_ld::{
    registration::{CSourceRegistration, RegistrationId, RegistrationInfo},
    subscription::{Activation, SubscriptionId},
};
use serde_json::{Value, json};

/// Pins: Check that instead of providing the original context source registration, implementations should return
/// context source registration information relevant for the subscription, in particular only matching
/// `RegistrationInfo` elements
/// Clause: 5.11.7
#[gtest]
#[tokio::test]
async fn e047_03_01_receive_csource_notification_with_relevant_information() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Building", "Bus"]).await?;
    let sink = NotificationSink::start().await?;
    let id: SubscriptionId = namespace.urn_text("Subscription", 1).parse()?;
    let uri = sink.endpoint(1)?;
    let subscription = building_registration_subscription(Some(id.clone()), &uri, Activation::Unspecified, None)?;
    let _subscription = create_registration_subscription(&ld_json_body_client(&broker, &vocabulary)?, &subscription, &uri).await?;
    let endpoint = csource_endpoint(&namespace, "my.csource.org:1026").await?;
    let registration_id: RegistrationId = namespace.urn_text("ContextSourceRegistration", 1).parse()?;
    let registration = CSourceRegistration::builder(endpoint.as_str())
        .id(registration_id.clone())
        .information([
            RegistrationInfo::builder().entities(["Building"]).build()?,
            RegistrationInfo::builder().entities(["Bus"]).build()?,
        ])
        .build()?;
    let _registration = registered_any(&broker, &vocabulary, &registration).await?;

    let notification = sink.wait_for(1, 1).await?;
    let body = notification.json()?;
    expect_csource_notification(&body, &id, &registration_id, "newlyMatching");
    let information = body.pointer("/data/0/information").and_then(Value::as_array);
    expect_that!(information.map(Vec::len), some(eq(1)));
    let first_type = information.and_then(|items| items.first()).and_then(|item| item.pointer("/entities/0/type"));
    expect_that!(
        first_type.map(|entity_type| entity_type == &json!("Building") || entity_type.pointer("/0") == Some(&json!("Building"))),
        some(eq(true))
    );

    teardown(&broker, &registration_id, &id).await
}
