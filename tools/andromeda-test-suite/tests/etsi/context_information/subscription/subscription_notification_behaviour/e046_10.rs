//! A notification is sent as JSON, with the test vocabulary linked in a Link header and not in the body (clause 5.8.6).

use crate::{
    building_payloads::building_simple_attributes,
    context_information::subscription::subscription_notification_behaviour::notification_case::{air_quality_updated, created, subscribed, teardown},
};
use andromeda_test_suite::{broker, namespace::Namespace, notification_sink::NotificationSink, vocabulary::Vocabulary, wire::header_values};
use googletest::prelude::*;
use http::header::LINK;
use ngsi_ld::{
    http::{ContextDelivery, headers::Link},
    query::EntitySelector,
    subscription::{Endpoint, NotificationParams, Subscription, SubscriptionId},
};
use serde_json::Value;

/// Pins: The Notification shall be sent as JSON
/// Clause: 5.8.6
#[gtest]
#[tokio::test]
async fn e046_10_01_check_that_the_notification_is_sent_as_json() -> Result<()> {
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
    let endpoint = Endpoint::builder(sink.endpoint(1)?.as_str()).build()?;
    // The subscription owns its identifier and keeps it private; the teardown deletes it by the same one.
    let subscription = Subscription::builder()
        .id(id.clone())
        .entities([EntitySelector::new("Building".parse()?).with_ids(building.id.clone())])
        .notification(NotificationParams::builder(endpoint).build()?)
        .build()?;
    let _subscription = subscribed(&as_list, &subscription).await?;

    air_quality_updated(&as_list, &building).await?;
    let notification = sink.wait_for(1, 1).await?;
    let body = notification.json()?;
    expect_that!(body.pointer("/data/0"), some(predicate(Value::is_object)));
    expect_that!(body.pointer("/data/0/@context"), none());
    // The sink keeps no logged date, so the body identifies the journal entry the wait returned.
    let received = sink.received(1).await?;
    let bodies = received.into_iter().map(|entry| entry.body).collect::<Vec<_>>();
    expect_that!(bodies, contains(eq(&notification.body)));

    let links = header_values(&notification.headers, &LINK)
        .into_iter()
        .map(|line| Ok(line.parse::<Link>()?))
        .collect::<Result<Vec<_>>>()?;
    let link_values = links.iter().flat_map(Link::values).collect::<Vec<_>>();
    expect_that!(link_values.len(), eq(1));
    let context_link = link_values.first().filter(|value| value.is_context_link());
    expect_that!(context_link.map(|value| value.target().as_str()), some(eq(vocabulary.url().as_str())));
    expect_that!(
        context_link.and_then(|value| value.media_type()).map(ToString::to_string),
        some(eq("application/ld+json"))
    );

    teardown(&broker, &id, &building).await;
    Ok(())
}
