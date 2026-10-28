//! ETSI `046_08`: a subscription without `notification.attributes` notifies every attribute of the entity, in the normalized
//! and in the keyValues format (clauses 5.8.6 and 5.3.1).

use crate::{
    context_information::subscription::subscription_notification_behaviour::notification_case::{air_quality_updated, created, subscribed, teardown},
    suite_keywords::{create_subscription_document, simplified_notification},
    suite_payloads::{building_simple_attributes, key_values_document},
};
use andromeda_test_suite::{broker, namespace::Namespace, notification_sink::NotificationSink, robot_literal, vocabulary::Vocabulary};
use googletest::prelude::*;
use ngsi_ld::{
    http::ContextDelivery,
    model::{NgsiLdMediaType, Representation},
    query::EntitySelector,
    subscription::{Activation, Endpoint, NotificationParams, Subscription, SubscriptionId},
};
use serde_json::{Value, json};
use std::time::Duration;
use tokio::time::sleep;

/// Pins: The structure of the notification message shall be as mandated by clause 5.3.1. The absence of the
/// notification.attributes member of a Subscription means that all Entity Attributes shall be included. All attributes are
/// included
/// Clause: 5.8.6
/// Changed: Wait for notification parses the body as json.loads('''<body>'''), a Python '''...''' literal, so a backslash
/// in the body is read as an escape and a ''' or a final ' ends the literal; the port refuses all three (not contains
/// "\\", not contains "'''", not endsWith "'") and then requires the body to be JSON, which is stricter only where an
/// escape leaves the keyword reading, and accepting, a body other than the one sent
/// Changed: The non-empty `$.data` is required to be an array, which the normalized notification always carries; the Hurl
/// line also accepts an object or a string
#[gtest]
#[tokio::test]
async fn e046_08_01_check_that_a_notification_is_sent_with_all_attributes() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Building", "name", "subCategory", "airQualityLevel", "almostFull"]).await?;
    let sink = NotificationSink::start().await?;
    let as_string = broker.with_context(vocabulary.context())?.with_context_delivery(ContextDelivery::LdJsonBody);
    let as_list = broker
        .with_context(vocabulary.context_list())?
        .with_context_delivery(ContextDelivery::LdJsonBody);
    let building = building_simple_attributes(&namespace, 1)?;
    created(&as_string, &building).await?;

    let id: SubscriptionId = namespace.urn_text("Subscription", 1).parse()?;
    let endpoint = Endpoint::builder(sink.endpoint(1)?.as_str()).accept(NgsiLdMediaType::Json).build()?;
    // The subscription owns its identifier and keeps it private; the teardown deletes it by the same one.
    let subscription = Subscription::builder()
        .id(id.clone())
        .activation(Activation::Active)
        .entities([EntitySelector::new("Building".parse()?).with_ids(building.id.clone())])
        .notification(NotificationParams::builder(endpoint).format(Representation::Normalized).build()?)
        .build()?;
    let _subscription = subscribed(&as_list, &subscription).await?;

    air_quality_updated(&as_list, &building).await?;
    let notification = sink.wait_for(1, 1).await?;
    let body = robot_literal::read(&notification.body)?;
    expect_that!(body.pointer("/subscriptionId"), some(eq(&json!(id.to_string()))));
    expect_that!(body.pointer("/data").and_then(Value::as_array).map(Vec::len), some(gt(0)));
    expect_that!(body.pointer("/data/0/id"), some(eq(&json!(building.id.to_string()))));
    expect_that!(body.pointer("/data/0/name/value"), some(eq(&json!("Eiffel Tower"))));
    expect_that!(body.pointer("/data/0/almostFull"), some(anything()));
    expect_that!(body.pointer("/data/0/airQualityLevel"), some(anything()));
    expect_that!(body.pointer("/data/0/subCategory"), some(anything()));

    teardown(&broker, &id, &building).await;
    Ok(())
}

/// Pins: The structure of the notification message shall be as mandated by clause 5.3.1. The absence of the
/// notification.attributes member of a Subscription means that all Entity Attributes shall be included
/// Clause: 5.8.6
/// Changed: Wait for notification parses the body as json.loads('''<body>'''), a Python '''...''' literal, so a backslash
/// in the body is read as an escape and a ''' or a final ' ends the literal; the port refuses all three (not contains
/// "\\", not contains "'''", not endsWith "'") and then requires the body to be JSON, which is stricter only where an
/// escape leaves the keyword reading, and accepting, a body other than the one sent
/// Changed: The non-empty `$.data` is required to be an array, which the normalized notification always carries; the Hurl
/// line also accepts an object or a string
#[gtest]
#[tokio::test]
async fn e046_08_02_check_that_a_notification_is_sent_with_all_attributes_in_simplified_format() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Building", "name", "subCategory", "airQualityLevel", "almostFull"]).await?;
    let sink = NotificationSink::start().await?;
    let as_string = broker.with_context(vocabulary.context())?.with_context_delivery(ContextDelivery::LdJsonBody);
    let as_list = broker
        .with_context(vocabulary.context_list())?
        .with_context_delivery(ContextDelivery::LdJsonBody);
    let building = building_simple_attributes(&namespace, 1)?;
    created(&as_string, &building).await?;

    let id: SubscriptionId = namespace.urn_text("Subscription", 1).parse()?;
    let endpoint_url = sink.endpoint(1)?;
    // The subscription owns its identifier and keeps it private; the teardown deletes it by the same one.
    let subscription = Subscription::builder()
        .id(id.clone())
        .activation(Activation::Active)
        .entities([EntitySelector::new("Building".parse()?).with_ids(building.id.clone())])
        .notification(simplified_notification(&endpoint_url)?)
        .build()?;
    let _subscription = create_subscription_document(&as_list, &subscription, &key_values_document(&subscription, &endpoint_url)?).await?;
    sleep(Duration::from_secs(1)).await;

    air_quality_updated(&as_list, &building).await?;
    let notification = sink.wait_for(1, 1).await?;
    let body = robot_literal::read(&notification.body)?;
    expect_that!(body.pointer("/subscriptionId"), some(eq(&json!(id.to_string()))));
    expect_that!(body.pointer("/data").and_then(Value::as_array).map(Vec::len), some(gt(0)));
    expect_that!(body.pointer("/data/0/id"), some(eq(&json!(building.id.to_string()))));
    expect_that!(body.pointer("/data/0/name"), some(eq(&json!("Eiffel Tower"))));

    teardown(&broker, &id, &building).await;
    Ok(())
}
