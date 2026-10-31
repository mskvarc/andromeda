//! Deleting a watched attribute notifies a normalized subscription with the `attributeDeleted` trigger and `sysAttrs`,
//! the attribute reported with its `deletedAt` (clause 5.8.6).

use crate::{
    building_payloads::{BUILDING_SIMPLE_TERMS, building_simple_attributes},
    context_information::subscription::subscription_notification_behaviour::notification_case::{subscription_created, teardown},
    entity_operations::{create_entity, ld_json_body_client},
};
use andromeda_test_suite::{broker, namespace::Namespace, notification_sink::NotificationSink, vocabulary::Vocabulary};
use googletest::prelude::*;
use ngsi_ld::{
    model::{NgsiLdMediaType, Representation},
    query::EntitySelector,
    subscription::{Endpoint, NotificationParams, NotificationTrigger, Subscription, SubscriptionId, SystemAttributeReporting},
};
use serde_json::{Value, json};

/// Pins: Delete an attribute and check the received notification
/// Clause: 5.8.6
#[gtest]
#[tokio::test]
async fn e046_22_05_check_that_a_notification_is_sent_with_matching_entity() -> Result<()> {
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
        .notification(
            NotificationParams::builder(endpoint)
                .format(Representation::Normalized)
                .system_attributes(SystemAttributeReporting::Include)
                .build()?,
        )
        .notification_triggers([NotificationTrigger::AttributeDeleted])
        .build()?;
    let _subscription = subscription_created(&ld_json_body_client(&broker, &vocabulary)?, &subscription).await?;
    let sink = sink.start_serving().await?;

    let as_link = broker.with_context(vocabulary.context())?;
    as_link.entities().delete_attribute(&building.id, &"name".parse()?).send().await?;

    let notification = sink.wait_for(1, 1).await?;
    let body = notification.json()?;
    expect_that!(body.pointer("/subscriptionId"), some(eq(&json!(id.to_string()))));
    expect_that!(body.pointer("/data").and_then(Value::as_array).map(Vec::len), some(eq(1)));
    expect_that!(body.pointer("/data/0/id"), some(eq(&json!(building.id.to_string()))));
    expect_that!(body.pointer("/data/0/name/deletedAt"), some(anything()));

    teardown(&broker, &id, &building).await;
    Ok(())
}
