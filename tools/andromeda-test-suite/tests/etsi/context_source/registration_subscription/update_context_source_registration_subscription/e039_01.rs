//! An update of the notification of a Context Source Registration Subscription is retrieved with the new endpoint
//! (clause 5.11.3).

use crate::{
    entity_operations::ld_json_body_client,
    registration_subscription_operations::{
        REGISTRATION_SUBSCRIPTION_STATE,
        create_registration_subscription,
        registration_notification_fragment,
        registration_subscription_json,
        update_registration_subscription_request,
        without_members,
    },
    subscription_operations::subscription_updated,
    subscription_payloads::building_registration_subscription,
};
use andromeda_test_suite::{
    broker,
    comparison::matches_document,
    namespace::Namespace,
    notification_sink::NotificationSink,
    teardown::AnyAnswer,
    vocabulary::Vocabulary,
};
use googletest::prelude::*;
use ngsi_ld::{
    model::NgsiLdMediaType,
    subscription::{Activation, SubscriptionId},
};
use serde_json::json;

/// Pins: Check that one can update a context source registration subscription
/// Clause: 5.11.3
/// Expected: the created or updated registration subscription is compared with jsonldContext left out (clause 5.8.1.4)
#[gtest]
#[tokio::test]
async fn e039_01_01_update_context_source_registration_subscription() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Building"]).await?;
    let sink = NotificationSink::start().await?;
    let id: SubscriptionId = namespace.urn_text("Subscription", 1).parse()?;
    let uri = sink.endpoint(1)?;
    let subscription = building_registration_subscription(Some(id.clone()), &uri, Activation::Unspecified, None)?;
    let _subscription = create_registration_subscription(&ld_json_body_client(&broker, &vocabulary)?, &subscription, &uri).await?;

    let linked = broker.with_context(vocabulary.context())?;
    let updated_uri = sink.endpoint(2)?;
    let document = registration_notification_fragment(&updated_uri)?;
    subscription_updated(&linked, update_registration_subscription_request(&linked, &id, &document)?).await?;

    let body = registration_subscription_json(&linked.with_accept(NgsiLdMediaType::JsonLd), &id).await?;
    expect_that!(
        &without_members(body, &REGISTRATION_SUBSCRIPTION_STATE),
        matches_document(json!({
            "id": id.to_string(),
            "type": "Subscription",
            "entities": [{"type": "Building"}],
            "notification": {
                "format": "keyValues",
                "endpoint": {"uri": updated_uri.as_str(), "accept": "application/json"}
            },
            "@context": vocabulary.url().as_str(),
        }))
    );

    broker.registration_subscriptions().delete(&id).send().await.any_answer();
    Ok(())
}
