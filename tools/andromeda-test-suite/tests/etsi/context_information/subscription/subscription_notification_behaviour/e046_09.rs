//! A notification is sent to the endpoint the subscription names, with its `receiverInfo` fields (clauses 5.8.6 and
//! 5.2.22).

use crate::{
    building_payloads::building_simple_attributes,
    context_information::subscription::subscription_notification_behaviour::notification_case::{air_quality_updated, created, subscribed, teardown},
};
use andromeda_test_suite::{broker, namespace::Namespace, notification_sink::NotificationSink, vocabulary::Vocabulary};
use googletest::prelude::*;
use ngsi_ld::{
    http::ContextDelivery,
    model::{NgsiLdMediaType, Representation},
    query::EntitySelector,
    subscription::{Endpoint, NotificationParams, Subscription, SubscriptionId},
};

/// Pins: A Notification shall be sent (as mandated by each concrete binding and including any optional
/// endpoint.receiverInfo defined by clause 5.2.22) to the endpoint specified by the endpoint.uri member of the notification
/// structure defined by clause 5.2.1
/// Clause: 5.8.6
#[gtest]
#[tokio::test]
async fn e046_09_01_check_that_a_notification_is_sent_to_the_endpoint() -> Result<()> {
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
    let endpoint = Endpoint::builder(sink.endpoint(1)?.as_str())
        .accept(NgsiLdMediaType::Json)
        .receiver_info("X-Additional-Key", "Value")
        .build()?;
    // The subscription owns its identifier and keeps it private; the teardown deletes it by the same one.
    let subscription = Subscription::builder()
        .id(id.clone())
        .entities([EntitySelector::new("Building".parse()?).with_ids(building.id.clone())])
        .notification(NotificationParams::builder(endpoint).format(Representation::Normalized).build()?)
        .build()?;
    let _subscription = subscribed(&as_list, &subscription).await?;

    air_quality_updated(&as_list, &building).await?;
    let notification = sink.wait_for(1, 1).await?;
    notification.json()?;
    expect_that!(notification.headers.get("x-additional-key"), some(anything()));
    // The sink keeps no logged date, so the body identifies the journal entry the wait returned.
    let received = sink.received(1).await?;
    let bodies = received.into_iter().map(|entry| entry.body).collect::<Vec<_>>();
    expect_that!(bodies, contains(eq(&notification.body)));

    teardown(&broker, &id, &building).await;
    Ok(())
}
