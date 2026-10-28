//! ETSI `029_03`: an update whose fragment breaks the data types or restrictions of a subscription is refused with 400
//! `BadRequestData` (clause 5.8.2).

use crate::suite_keywords::{create_timed_building_subscription, ld_json_body_client, refused, update_subscription_request};
use andromeda_test_suite::{broker, namespace::Namespace, notification_sink::NotificationSink, teardown::AnyAnswer, vocabulary::Vocabulary};
use googletest::prelude::*;
use ngsi_ld::{
    http::UncheckedDocument,
    model::{ErrorType, RawJson},
    subscription::{SubscriptionFragment, SubscriptionId},
};

/// Pins: Check that one cannot update a subscription: If the data types and restriction are not met by the
/// Subscription Fragment, then an error of type `BadRequestData` shall be raised
/// Clause: 5.8.2
#[gtest]
#[tokio::test]
async fn e029_03_01_update_subscription_with_invalid_fragment() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Building"]).await?;
    let sink = NotificationSink::start().await?;
    let id: SubscriptionId = namespace.urn_text("Subscription", 1).parse()?;
    let client = ld_json_body_client(&broker, &vocabulary)?;
    let _subscription = create_timed_building_subscription(&client, &id, &sink.endpoint(1)?).await?;

    let notification: RawJson = r#"{"format":"keyValues"}"#.parse()?;
    let document = UncheckedDocument::try_from(SubscriptionFragment::builder().build()?.render())?
        .remove_member("type")
        .set_member("notification", &notification);
    refused(&broker, update_subscription_request(&broker, &id, &document)?, ErrorType::BadRequestData).await?;

    broker.subscriptions().delete(&id).send().await.any_answer();
    Ok(())
}
