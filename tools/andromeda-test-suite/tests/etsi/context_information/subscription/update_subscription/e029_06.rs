//! An update replaces the notification of a subscription, and the subscription keeps the replacement (clause 5.8.2).

use crate::{
    entity_operations::ld_json_body_client,
    subscription_operations::{create_timed_building_subscription, key_values_fragment, subscription_updated, update_subscription_request},
};
use andromeda_test_suite::{
    broker,
    comparison::matches_document,
    namespace::Namespace,
    notification_sink::NotificationSink,
    teardown::AnyAnswer,
    vocabulary::Vocabulary,
    wire::json_body,
};
use googletest::prelude::*;
use ngsi_ld::{model::NgsiLdMediaType, subscription::SubscriptionId};
use serde_json::json;

/// Pins: Check that one can update a subscription: The implementation shall modify the target Subscription
/// Clause: 5.8.2
#[gtest]
#[tokio::test]
async fn e029_06_01_update_subscription() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Building"]).await?;
    let sink = NotificationSink::start().await?;
    let id: SubscriptionId = namespace.urn_text("Subscription", 1).parse()?;
    let client = ld_json_body_client(&broker, &vocabulary)?;
    let _subscription = create_timed_building_subscription(&client, &id, &sink.endpoint(1)?).await?;

    let linked = broker.with_context(vocabulary.context())?;
    let uri = sink.endpoint(2)?;
    subscription_updated(&linked, update_subscription_request(&linked, &id, &key_values_fragment(&uri)?)?).await?;

    let received = linked
        .with_accept(NgsiLdMediaType::JsonLd)
        .with_responses()
        .subscriptions()
        .retrieve(&id)
        .send()
        .await?;
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
                "endpoint": {"uri": uri.as_str(), "accept": "application/json"}
            },
            "@context": context,
        }))
        .hiding_everywhere("status")
        .hiding_everywhere("jsonldContext")
        .hiding_everywhere("timesFailed")
        .hiding_everywhere("timesSent")
        .hiding_everywhere("notificationTrigger")
    );

    broker.subscriptions().delete(&id).send().await.any_answer();
    Ok(())
}
