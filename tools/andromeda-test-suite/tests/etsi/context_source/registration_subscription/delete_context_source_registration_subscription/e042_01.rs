//! ETSI `042_01`: a deleted Context Source Registration Subscription is no longer found (clause 5.11.6).

use crate::{
    suite_keywords::{create_registration_subscription, expect_batch_done, ld_json_body_client},
    suite_payloads::building_registration_subscription,
};
use andromeda_test_suite::{broker, namespace::Namespace, notification_sink::NotificationSink, problem::answered, vocabulary::Vocabulary};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    model::NgsiLdMediaType,
    subscription::{Activation, SubscriptionId},
};

/// Pins: Check that one can delete a context source registration subscription
/// Clause: 5.11.6
#[gtest]
#[tokio::test]
async fn e042_01_01_delete_context_source_registration_subscription() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Building"]).await?;
    let sink = NotificationSink::start().await?;
    let id: SubscriptionId = namespace.urn_text("Subscription", 1).parse()?;
    let uri = sink.endpoint(1)?;
    let subscription = building_registration_subscription(Some(id.clone()), &uri, Activation::Unspecified, None)?;
    let _subscription = create_registration_subscription(&ld_json_body_client(&broker, &vocabulary)?, &subscription, &uri).await?;

    expect_batch_done(broker.with_responses().registration_subscriptions().delete(&id).send().await)?;

    let reader = broker.with_context(vocabulary.context())?.with_accept(NgsiLdMediaType::Json);
    let retrieved = reader.with_responses().registration_subscriptions().retrieve(&id).send().await;
    verify_that!(&retrieved, answered(StatusCode::NOT_FOUND))
}
