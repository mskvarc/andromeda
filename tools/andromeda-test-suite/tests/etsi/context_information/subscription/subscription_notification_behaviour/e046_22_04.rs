//! ETSI `046_22_04`: deleting a watched attribute notifies a normalized subscription with the `attributeDeleted`
//! trigger, the attribute reported as a Property whose value is the NGSI-LD Null (clause 5.8.6).

use crate::{
    context_information::subscription::subscription_notification_behaviour::{
        deletion_case::NGSI_LD_NULL,
        notification_case::{subscription_created, teardown},
    },
    suite_keywords::{create_entity, ld_json_body_client},
    suite_payloads::{BUILDING_SIMPLE_TERMS, building_simple_attributes},
};
use andromeda_test_suite::{
    broker,
    comparison::matches_document,
    namespace::Namespace,
    notification_sink::NotificationSink,
    robot_literal,
    vocabulary::Vocabulary,
};
use googletest::prelude::*;
use ngsi_ld::{
    model::{NgsiLdMediaType, Representation},
    query::EntitySelector,
    subscription::{Endpoint, NotificationParams, NotificationTrigger, Subscription, SubscriptionId},
};
use serde_json::{Value, json};

/// Pins: Delete an attribute and check the received notification
/// Clause: 5.8.6
/// Changed: Wait for notification parses the body as json.loads('''<body>'''), a Python '''...''' literal, so a backslash
/// in the body is read as an escape and a ''' or a final ' ends the literal; the port refuses all three (not contains
/// "\\", not contains "'''", not endsWith "'") and then requires the body to be JSON, which is stricter only where an
/// escape leaves the keyword reading, and accepting, a body other than the one sent
/// Changed: The `length` check requires `$.data` to be an array, which a notification always carries; the Hurl line also
/// accepts a string or an object of that length
#[gtest]
#[tokio::test]
async fn e046_22_04_check_that_a_notification_is_sent_with_matching_entity() -> Result<()> {
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

    let as_link = broker.with_context(vocabulary.context())?;
    as_link.entities().delete_attribute(&building.id, &"name".parse()?).send().await?;

    let notification = sink.wait_for(1, 1).await?;
    let body = robot_literal::read(&notification.body)?;
    expect_that!(body.pointer("/subscriptionId"), some(eq(&json!(id.to_string()))));
    expect_that!(body.pointer("/data").and_then(Value::as_array).map(Vec::len), some(eq(1)));
    expect_that!(body.pointer("/data/0/id"), some(eq(&json!(building.id.to_string()))));
    expect_that!(
        body.pointer("/data/0"),
        some(
            matches_document(json!({
                "type": "Building",
                "almostFull": {"type": "Property", "value": false},
                "subCategory": {"type": "Property", "value": "tourism"},
                "airQualityLevel": {
                    "type": "Property",
                    "value": 4,
                    "unitCode": "C62",
                    "observedAt": "2020-09-09T16:40:00.000Z",
                },
                "name": {"type": "Property", "value": NGSI_LD_NULL},
            }))
            .hiding_everywhere("id")
        )
    );

    teardown(&broker, &id, &building).await;
    Ok(())
}
