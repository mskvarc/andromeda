//! ETSI `032_03`: a subscription is deleted with no content in the answer, and is not retrieved afterwards (clause 5.8.5).

use crate::{
    suite_keywords::{create_subscription_document, expect_batch_done, ld_json_body_client},
    suite_payloads::key_values_document,
};
use andromeda_test_suite::{broker, namespace::Namespace, notification_sink::NotificationSink, problem::answered, vocabulary::Vocabulary};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    model::{NgsiLdMediaType, Representation},
    query::EntitySelector,
    subscription::{Endpoint, NotificationParams, Subscription, SubscriptionId},
};

/// Pins: Check that one can delete a subscription
/// Clause: 5.8.5
#[gtest]
#[tokio::test]
async fn e032_03_01_delete_subscription() -> Result<()> {
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

    let answer = broker.with_responses().subscriptions().delete(&id).send().await;
    expect_batch_done(answer)?;

    let retrieved = broker.with_responses().subscriptions().retrieve(&id).send().await;
    verify_that!(&retrieved, answered(StatusCode::NOT_FOUND))
}
