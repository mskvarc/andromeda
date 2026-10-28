//! ETSI `046_22_12`: deleting an entity notifies a normalized subscription watching one of its attributes with the
//! `attributeDeleted` trigger, the entity reported with its `deletedAt` only (clause 5.8.6).

use crate::{
    context_information::subscription::subscription_notification_behaviour::notification_case::{ANY_SYSTEM_TIME, subscription_created, teardown},
    suite_keywords::{create_entity, ld_json_body_client},
    suite_payloads::{BUILDING_SIMPLE_TERMS, building_simple_attributes},
};
use andromeda_test_suite::{
    broker,
    comparison::matches_document,
    namespace::Namespace,
    notification_sink::NotificationSink,
    robot_literal,
    teardown::AnyAnswer,
    vocabulary::Vocabulary,
};
use googletest::prelude::*;
use ngsi_ld::{
    model::{NgsiLdMediaType, Representation},
    query::EntitySelector,
    subscription::{Endpoint, NotificationParams, NotificationTrigger, Subscription, SubscriptionId},
};
use serde_json::{Value, json};

/// Pins: Delete an entity with a matching attribute and check the received notification
/// Clause: 5.8.6
/// Corrected: fix(`046_22`): entity-deletion notification carries only deletedat (5.8.6)
/// Changed: Wait for notification parses the body as json.loads('''<body>'''), a Python '''...''' literal, so a backslash
/// in the body is read as an escape and a ''' or a final ' ends the literal; the port refuses all three (not contains
/// "\\", not contains "'''", not endsWith "'") and then requires the body to be JSON, which is stricter only where an
/// escape leaves the keyword reading, and accepting, a body other than the one sent
/// Changed: The `length` check requires `$.data` to be an array, which a notification always carries; the Hurl line also
/// accepts a string or an object of that length
#[gtest]
#[tokio::test]
async fn e046_22_12_check_that_a_notification_is_sent_with_matching_entity() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &BUILDING_SIMPLE_TERMS).await?;
    let sink = NotificationSink::refusing()?;
    let building = building_simple_attributes(&namespace, "None")?;
    create_entity(&broker, &vocabulary, &building).await?;

    let id: SubscriptionId = namespace.urn_text("Subscription", 1).parse()?;
    let endpoint = Endpoint::builder(sink.endpoint(1)?.as_str()).accept(NgsiLdMediaType::Json).build()?;
    // The subscription owns its identifier and keeps it private; the teardown deletes it by the same one.
    let subscription = Subscription::builder()
        .id(id.clone())
        .entities([EntitySelector::new("Building".parse()?)])
        .watched_attributes(["name"])
        .notification(NotificationParams::builder(endpoint).format(Representation::Normalized).build()?)
        .notification_triggers([NotificationTrigger::AttributeDeleted])
        .build()?;
    let _subscription = subscription_created(&ld_json_body_client(&broker, &vocabulary)?, &subscription).await?;
    let sink = sink.start_serving().await?;

    broker.entities().delete(&building.id).send().await?;
    broker.temporal().delete(&building.id).send().await.any_answer();

    let notification = sink.wait_for(1, 1).await?;
    let body = robot_literal::read(&notification.body)?;
    expect_that!(body.pointer("/subscriptionId"), some(eq(&json!(id.to_string()))));
    expect_that!(body.pointer("/data").and_then(Value::as_array).map(Vec::len), some(eq(1)));
    expect_that!(body.pointer("/data/0/id"), some(eq(&json!(building.id.to_string()))));
    expect_that!(
        body.pointer("/data/0"),
        some(matches_document(json!({"type": "Building", "deletedAt": ANY_SYSTEM_TIME})).hiding_everywhere("id"))
    );

    teardown(&broker, &id, &building).await;
    Ok(())
}
