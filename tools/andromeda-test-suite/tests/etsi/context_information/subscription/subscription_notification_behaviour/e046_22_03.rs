//! Deleting an attribute the subscription does not watch sends no notification (clause 5.8.6).

use crate::{
    building_payloads::{BUILDING_SIMPLE_TERMS, building_simple_attributes},
    context_information::subscription::subscription_notification_behaviour::notification_case::teardown,
    entity_operations::{create_entity, ld_json_body_client},
    subscription_operations::{create_subscription_document, simplified_notification},
    subscription_payloads::key_values_document,
};
use andromeda_test_suite::{broker, namespace::Namespace, notification_sink::NotificationSink, vocabulary::Vocabulary};
use googletest::prelude::*;
use ngsi_ld::{
    query::EntitySelector,
    subscription::{NotificationTrigger, Subscription, SubscriptionId},
};
use std::time::Duration;

/// Pins: Delete a not watched attribute and check no notification is received
/// Clause: 5.8.6
/// Absence: no notification within 10 s
#[gtest]
#[tokio::test]
async fn e046_22_03_check_that_a_notification_is_not_sent_with_updated_entity() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &BUILDING_SIMPLE_TERMS).await?;
    let sink = NotificationSink::refusing()?;
    let building = building_simple_attributes(&namespace, "None")?;
    create_entity(&broker, &vocabulary, &building).await?;

    let id: SubscriptionId = namespace.urn_text("Subscription", 1).parse()?;
    let endpoint_url = sink.endpoint(1)?;
    // The subscription owns its identifier and keeps it private; the teardown deletes it by the same one.
    let subscription = Subscription::builder()
        .id(id.clone())
        .entities([EntitySelector::new("Building".parse()?)])
        .watched_attributes(["name"])
        .notification(simplified_notification(&endpoint_url)?)
        .notification_triggers([NotificationTrigger::AttributeDeleted])
        .build()?;
    let document = key_values_document(&subscription, &endpoint_url)?;
    let _subscription = create_subscription_document(&ld_json_body_client(&broker, &vocabulary)?, &subscription, &document).await?;
    let sink = sink.start_serving().await?;

    let as_link = broker.with_context(vocabulary.context())?;
    as_link.entities().delete_attribute(&building.id, &"airQualityLevel".parse()?).send().await?;

    expect_that!(sink.count_after(1, Duration::from_secs(10)).await?, eq(0));

    teardown(&broker, &id, &building).await;
    Ok(())
}
