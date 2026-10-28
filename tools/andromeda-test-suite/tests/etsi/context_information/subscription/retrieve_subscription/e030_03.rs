//! ETSI `030_03`: a subscription is retrieved with the members it was created with, in the key-value form (clause 5.8.3).

use crate::{
    suite_keywords::{create_subscription_document, ld_json_body_client},
    suite_payloads::key_values_document,
};
use andromeda_test_suite::{
    broker,
    comparison::matches_document,
    namespace::Namespace,
    notification_sink::NotificationSink,
    problem::answered,
    teardown::AnyAnswer,
    vocabulary::Vocabulary,
    wire::json_body,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{NgsiLdMediaType, Representation},
    query::EntitySelector,
    subscription::{Endpoint, NotificationParams, Subscription, SubscriptionId},
};
use serde_json::json;

/// Pins: Check that one can retrieve a subscription
/// Clause: 5.8.3
#[gtest]
#[tokio::test]
async fn e030_03_01_retrieve_subscription() -> Result<()> {
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
    let client = ld_json_body_client(&broker, &vocabulary)?;
    let _subscription = create_subscription_document(&client, &subscription, &key_values_document(&subscription, &endpoint_url)?).await?;

    let retrieved = broker
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
        .with_responses()
        .subscriptions()
        .retrieve(&id)
        .send()
        .await;
    verify_that!(&retrieved, answered(StatusCode::OK))?;
    let body = json_body(&retrieved?)?;
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
        }))
        .hiding_everywhere("@context")
        .hiding_everywhere("jsonldContext")
        .hiding_everywhere("timesFailed")
        .hiding_everywhere("timesSent")
        .hiding_everywhere("notificationTrigger")
    );

    broker.subscriptions().delete(&id).send().await.any_answer();
    Ok(())
}
