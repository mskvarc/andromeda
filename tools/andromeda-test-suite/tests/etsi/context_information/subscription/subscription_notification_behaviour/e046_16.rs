//! ETSI `046_16`: a subscription whose entity type selection is the disjunction `Building|Tower` notifies the creation
//! of an entity that matches it and only such a one (clauses 5.8.6 and 4.17).

use crate::suite_payloads::{building_simple_attributes, vehicle_parked};
use andromeda_test_suite::{
    broker::{self, BrokerClient},
    cleanup::Cleanup,
    namespace::Namespace,
    notification_sink::NotificationSink,
    problem::answered,
    robot_literal,
    teardown::AnyAnswer,
    vocabulary::Vocabulary,
    wire::send_to,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{
        ContextDelivery,
        operation::{CreateEntity, CreateSubscription},
    },
    model::{Entity, Input},
    query::{EntitySelector, SelectorType},
    subscription::{Endpoint, NotificationParams, NotificationTrigger, Subscription, SubscriptionId},
};
use serde_json::json;
use std::time::Duration;
use tokio::time::sleep;

/// Subscribes, after the case's one-second sleep, to the creation of entities of type `Building|Tower`, notified to
/// endpoint 1 of `sink`, and answers the subscription's identifier and the cleanup that deletes it.
async fn subscribed(client: &BrokerClient, namespace: &Namespace, sink: &NotificationSink) -> Result<(SubscriptionId, Cleanup)> {
    sleep(Duration::from_secs(1)).await;
    let id: SubscriptionId = namespace.urn_text("Subscription", 1).parse()?;
    // The subscription owns its identifier and keeps it private; the caller deletes it by the same one.
    let subscription = Subscription::builder()
        .id(id.clone())
        .entities([EntitySelector::new("Building|Tower".parse::<SelectorType>()?)])
        .notification_triggers([NotificationTrigger::EntityCreated])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?.as_str()).build()?).build()?)
        .build()?;
    let cleanup = Cleanup::of(client).subscription_of(&subscription);
    let created = send_to(client, &CreateSubscription { subscription: &subscription }, "subscriptions/").await;
    verify_that!(&created, answered(StatusCode::CREATED))?;
    Ok((id, cleanup))
}

/// Creates `entity` as the case does, its answer unchecked.
async fn create(client: &BrokerClient, entity: &Entity<Input>) {
    send_to(client, &CreateEntity::builder().entity(entity).build(), "entities/").await.any_answer();
}

/// Deletes what the case created.
async fn teardown(broker: &BrokerClient, subscription: &SubscriptionId, entity: &Entity<Input>) {
    broker.subscriptions().delete(subscription).send().await.any_answer();
    broker.entities().delete(&entity.id).send().await.any_answer();
    broker.temporal().delete(&entity.id).send().await.any_answer();
}

/// Pins: If a subscription defines an entity type selection query, a notification shall be sent whenever an entity
/// matches the query.
/// Clause: 5.8.6
/// Corrected: fix(`046_16_01`, `046_16_02`): select building or tower with the disjunction the type selection grammar
/// defines (4.17)
/// Changed: Wait for notification parses the body as json.loads('''<body>'''), a Python '''...''' literal; the port
/// refuses a backslash, a ''' and a final ' and then requires JSON, which is stricter only where an escape leaves the
/// keyword reading, and accepting, a body other than the one sent
#[gtest]
#[tokio::test]
async fn e046_16_01_check_that_a_notification_is_sent_with_entity_matching_the_entity_type_selection() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(
        &broker,
        &namespace,
        &["Building", "name", "subCategory", "airQualityLevel", "almostFull", "Tower"],
    )
    .await?;
    let client = broker
        .with_context(vocabulary.context_list())?
        .with_context_delivery(ContextDelivery::LdJsonBody);
    let sink = NotificationSink::start().await?;
    let (subscription, _subscription) = subscribed(&client, &namespace, &sink).await?;
    let building = building_simple_attributes(&namespace, 1)?;
    create(&client, &building).await;

    let notification = sink.wait_for(1, 1).await?;
    let body = robot_literal::read(&notification.body)?;
    expect_that!(body.pointer("/subscriptionId"), some(eq(&json!(subscription.to_string()))));
    expect_that!(body.pointer("/data").and_then(|data| data.as_array()).map(Vec::len), some(gt(0)));
    expect_that!(body.pointer("/data/0/id"), some(eq(&json!(building.id.to_string()))));
    teardown(&broker, &subscription, &building).await;
    Ok(())
}

/// Pins: If a subscription defines an entity type selection query, a notification shall not be sent if the entity
/// type does not match the query
/// Clause: 5.8.6
/// Corrected: fix(`046_16_01`, `046_16_02`): select building or tower with the disjunction the type selection grammar
/// defines (4.17)
/// Absence: Wait for no notification 5 s
#[gtest]
#[tokio::test]
async fn e046_16_02_check_that_a_notification_is_not_sent_if_the_entity_type_does_not_match_the_entity_type_selection() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Vehicle", "brandName", "isParked", "providedBy", "Building", "Tower"]).await?;
    let client = broker
        .with_context(vocabulary.context_list())?
        .with_context_delivery(ContextDelivery::LdJsonBody);
    let sink = NotificationSink::start().await?;
    let (subscription, _subscription) = subscribed(&client, &namespace, &sink).await?;
    let vehicle = vehicle_parked(&namespace, 1)?;
    create(&client, &vehicle).await;

    expect_that!(sink.count_after(1, Duration::from_secs(5)).await?, eq(0));
    teardown(&broker, &subscription, &vehicle).await;
    Ok(())
}
