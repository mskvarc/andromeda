//! An update of a subscription that no subscription holds is refused with 404 `ResourceNotFound` (clause 5.8.2).

use crate::{
    subscription_operations::{key_values_fragment, update_subscription_request},
    unchecked_requests::not_found,
};
use andromeda_test_suite::{broker, namespace::Namespace, notification_sink::NotificationSink};
use googletest::prelude::*;
use ngsi_ld::subscription::SubscriptionId;

/// Pins: Check that one cannot update a subscription: If the NGSI-LD System does not know about the target
/// Subscription, because there is no existing Subscription whose id (URI) is equivalent, an error of type
/// `ResourceNotFound` shall be raised
/// Clause: 5.8.2
#[gtest]
#[tokio::test]
async fn e029_02_01_update_unknown_subscription() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let id: SubscriptionId = namespace.urn_text("Subscription", "unknowSubscription").parse()?;
    let document = key_values_fragment(&sink.endpoint(2)?)?;
    not_found(&broker, update_subscription_request(&broker, &id, &document)?).await
}
