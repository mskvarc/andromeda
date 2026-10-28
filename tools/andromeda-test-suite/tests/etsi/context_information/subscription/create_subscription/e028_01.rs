//! ETSI `028_01`: a subscription created with a time interval and a key-value notification is retrieved with the members it
//! was created with (clause 5.8.1).

use crate::suite_payloads::key_values_document;
use andromeda_test_suite::{
    broker,
    cleanup::Cleanup,
    comparison::matches_document,
    namespace::Namespace,
    notification_sink::NotificationSink,
    teardown::AnyAnswer,
    vocabulary::Vocabulary,
    wire::json_body,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{ContextDelivery, operation::CreateSubscription},
    model::{NgsiLdMediaType, Representation},
    query::EntitySelector,
    subscription::{Endpoint, NotificationParams, Subscription, SubscriptionId},
};
use serde_json::json;

/// Pins: Check that one can create a subscription
/// Clause: 5.8.1
#[gtest]
#[tokio::test]
async fn e028_01_01_create_subscription() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Building"]).await?;
    let sink = NotificationSink::start().await?;
    let id: SubscriptionId = namespace.urn_text("Subscription", 1).parse()?;
    let endpoint_url = sink.endpoint(1)?;
    let endpoint = Endpoint::builder(endpoint_url.as_str()).accept(NgsiLdMediaType::Json).build()?;
    let subscription = Subscription::builder()
        .id(id.clone())
        .time_interval(5)
        .entities([EntitySelector::new("Building".parse()?)])
        .notification(NotificationParams::builder(endpoint).format(Representation::Simplified).build()?)
        .build()?;
    let document = key_values_document(&subscription, &endpoint_url)?;
    let client = broker
        .with_context(vocabulary.context_list())?
        .with_context_delivery(ContextDelivery::LdJsonBody);
    let _subscription = Cleanup::of(&client).subscription_of(&subscription);
    let created = client.unchecked(&CreateSubscription { subscription: &subscription })?.document(&document);
    client.send_unchecked(created).await.any_answer();

    let client = broker.with_context(vocabulary.context())?.with_accept(NgsiLdMediaType::JsonLd).with_responses();
    let received = client.subscriptions().retrieve(&id).send().await?;
    let body = json_body(&received)?;
    let context = vocabulary.url().as_str();
    expect_that!(
        &body,
        matches_document(json!({
            "id": id.to_string(),
            "type": "Subscription",
            "timeInterval": 5,
            "entities": [{"type": "Building"}],
            "notification": {
                "format": "keyValues",
                "endpoint": {"uri": endpoint_url.as_str(), "accept": "application/json"}
            },
            "status": "active",
            "@context": context,
        }))
        .hiding_everywhere("jsonldContext")
        .hiding_everywhere("timesFailed")
        .hiding_everywhere("timesSent")
        .hiding_everywhere("notificationTrigger")
    );

    broker.subscriptions().delete(&id).send().await.any_answer();
    Ok(())
}
