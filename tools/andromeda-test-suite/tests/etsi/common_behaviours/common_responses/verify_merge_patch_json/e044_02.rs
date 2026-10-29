//! ETSI `044_02`: a PATCH of a subscription is accepted with `application/merge-patch+json` as its Content-Type
//! (clause 6.3.4).

use andromeda_test_suite::{
    broker::{self, BrokerClient},
    cleanup::Cleanup,
    namespace::Namespace,
    problem::answered,
    teardown::AnyAnswer,
    vocabulary::Vocabulary,
    wire::{content_type, send_to},
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{
        ContextDelivery,
        headers::ContentType,
        operation::{CreateSubscription, UpdateSubscription},
    },
    model::{NgsiLdMediaType, Representation},
    subscription::{Endpoint, NotificationParams, Subscription, SubscriptionFragment, SubscriptionId},
};

/// The client of `broker` sending `vocabulary` in the body, as the create case sends the suite's `@context`.
fn in_body(broker: &BrokerClient, vocabulary: &Vocabulary) -> Result<BrokerClient> {
    Ok(broker
        .with_context(vocabulary.context_list())?
        .with_context_delivery(ContextDelivery::LdJsonBody))
}

/// Pins: Verify that PATCH HTTP requests can be done with "application/merge-patch+json" as Content-Type
/// Clause: 6.3.4
/// Changed: the fragment omits the `type` member the Hurl body sends, since a subscription fragment carries no type.
#[gtest]
#[tokio::test]
async fn e044_02_01_patch_subscription() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Building"]).await?;
    let id: SubscriptionId = namespace.urn_text("Subscription", 1).parse()?;
    let endpoint = Endpoint::builder(format!("http://notifications.invalid/notify/{namespace}/1"))
        .accept(NgsiLdMediaType::Json)
        .build()?;
    let subscription = Subscription::builder()
        .id(id.clone())
        .time_interval(5)
        .entities(["Building"])
        .notification(NotificationParams::builder(endpoint).format(Representation::Simplified).build()?)
        .build()?;
    let _subscription = Cleanup::of(&broker).subscription_of(&subscription);
    let create = CreateSubscription { subscription: &subscription };
    let created = send_to(&in_body(&broker, &vocabulary)?, &create, "subscriptions/").await;
    verify_that!(&created, answered(StatusCode::CREATED))?;

    let client = broker.with_context(vocabulary.context())?;
    let endpoint = Endpoint::builder(format!("http://notifications.invalid/notify/{namespace}/2"))
        .accept(NgsiLdMediaType::Json)
        .build()?;
    let fragment = SubscriptionFragment::builder()
        .notification(NotificationParams::builder(endpoint).format(Representation::Simplified).build()?)
        .build()?;
    let update = UpdateSubscription { id: &id, fragment: &fragment };
    let answer = client
        .send_unchecked(content_type(client.unchecked(&update)?, &ContentType::MergePatch.media_type())?)
        .await;
    expect_that!(&answer, answered(StatusCode::NO_CONTENT));

    broker.subscriptions().delete(&id).send().await.any_answer();
    Ok(())
}
