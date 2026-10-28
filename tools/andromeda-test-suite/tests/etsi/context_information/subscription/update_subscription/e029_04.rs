//! ETSI `029_04`: an update that removes a mandatory member of a subscription, by setting it to null, is refused with
//! 400 `BadRequestData` (clause 5.8.2).

use crate::suite_keywords::{create_timed_building_subscription, ld_json_body_client, refused, update_subscription_request};
use andromeda_test_suite::{broker, namespace::Namespace, notification_sink::NotificationSink, teardown::AnyAnswer, vocabulary::Vocabulary};
use googletest::prelude::*;
use ngsi_ld::{
    http::UncheckedDocument,
    model::{ErrorType, RawJson},
    subscription::{SubscriptionFragment, SubscriptionId},
};

/// Pins: Check that one cannot update a subscription: Any attempt to remove (by setting them to null in the Fragment)
/// mandatory properties of a Subscription (clause 5.2.12) shall result in an error of type `BadRequestData`
/// Clause: 5.8.2
#[gtest]
#[tokio::test]
async fn e029_04_01_update_subscription_with_null_mandatory_properties() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Building"]).await?;
    let sink = NotificationSink::start().await?;
    let id: SubscriptionId = namespace.urn_text("Subscription", 1).parse()?;
    let client = ld_json_body_client(&broker, &vocabulary)?;
    let _subscription = create_timed_building_subscription(&client, &id, &sink.endpoint(1)?).await?;

    let notification: RawJson = r#"{"format":"keyValues","endpoint":null}"#.parse()?;
    let document = UncheckedDocument::try_from(SubscriptionFragment::builder().build()?.render())?.set_member("notification", &notification);
    refused(&broker, update_subscription_request(&broker, &id, &document)?, ErrorType::BadRequestData).await?;

    broker.subscriptions().delete(&id).send().await.any_answer();
    Ok(())
}
