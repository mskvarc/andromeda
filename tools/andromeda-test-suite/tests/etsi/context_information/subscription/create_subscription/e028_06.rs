//! ETSI `028_06`: a subscription created with a dataset identifier is retrieved with that identifier and its notification
//! triggers (clauses 5.8.1 and 4.5.5).

use crate::{suite_keywords::ld_json_body_client, suite_payloads::dataset};
use andromeda_test_suite::{
    broker,
    cleanup::Cleanup,
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
    model::{NgsiLdMediaType, Representation},
    query::EntitySelector,
    subscription::{Endpoint, NotificationParams, Subscription, SubscriptionId},
};
use serde_json::json;

/// Pins: Check that one can create a subscription with a datasetId
/// Clause: 5.8.1, 4.5.5
/// Corrected: fix(subscription-jsonldcontext): ignore jsonldcontext on retrieved subscriptions (5.8.1.4)
#[gtest]
#[tokio::test]
async fn e028_06_create_a_subscription_with_a_dataset_id() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Building"]).await?;
    let sink = NotificationSink::start().await?;
    let id: SubscriptionId = namespace.urn_text("Subscription", 1).parse()?;
    let endpoint_url = sink.endpoint(1)?;
    let endpoint = Endpoint::builder(endpoint_url.as_str()).accept(NgsiLdMediaType::JsonLd).build()?;
    let dataset_id = dataset(&namespace, "french-name")?;
    let subscription = Subscription::builder()
        .id(id.clone())
        .entities([EntitySelector::new("Building".parse()?)])
        .dataset_ids([dataset_id.clone()])
        .notification(NotificationParams::builder(endpoint).format(Representation::Normalized).build()?)
        .build()?;
    let client = ld_json_body_client(&broker, &vocabulary)?;
    let _subscription = Cleanup::of(&client).subscription_of(&subscription);
    client.subscriptions().create(&subscription).send().await?;

    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(NgsiLdMediaType::JsonLd)
        .with_responses()
        .subscriptions()
        .retrieve(&id)
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    let context = vocabulary.url().as_str();
    expect_that!(
        &body,
        matches_document(json!({
            "id": id.to_string(),
            "type": "Subscription",
            "entities": [{"type": "Building"}],
            "notification": {
                "format": "normalized",
                "endpoint": {"uri": endpoint_url.as_str(), "accept": "application/ld+json"}
            },
            "notificationTrigger": ["attributeCreated", "attributeUpdated"],
            "datasetId": [dataset_id.to_string()],
            "status": "active",
            "@context": context,
        }))
        .hiding_everywhere("jsonldContext")
    );

    broker.subscriptions().delete(&id).send().await.any_answer();
    Ok(())
}
