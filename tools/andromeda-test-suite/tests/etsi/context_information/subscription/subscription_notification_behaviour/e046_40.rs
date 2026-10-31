//! Creating an entity notifies a subscription with `showChanges` and the `entityCreated` trigger without any previous
//! value (clause 5.8.6).

use crate::{
    building_payloads::{BUILDING_SIMPLE_TERMS, building_simple_attributes},
    context_information::subscription::subscription_notification_behaviour::{
        change_case::{ContextForm, EntityChange, change_notification, show_changes_subscription},
        notification_case::{descendants, teardown},
    },
};
use andromeda_test_suite::{broker, namespace::Namespace};
use googletest::prelude::*;
use ngsi_ld::subscription::{NotificationTrigger, SubscriptionId};

/// Pins: Create entity and check that notification has no previous value
/// Clause: 5.8.6
#[gtest]
#[tokio::test]
async fn e046_40_01_create_entity_and_check_no_previous_value_in_notification() -> Result<()> {
    let namespace = Namespace::fresh();
    let id: SubscriptionId = namespace.urn_text("Subscription", 1).parse()?;
    let building = building_simple_attributes(&namespace, 1)?;
    let (notification, _subscription) = change_notification(
        &namespace,
        &BUILDING_SIMPLE_TERMS,
        &[],
        ContextForm::String,
        |uri| show_changes_subscription(&id, uri, Some(NotificationTrigger::EntityCreated)),
        EntityChange::CreateEntity(&building),
    )
    .await?;
    let body = notification.json()?;
    let data = body.get("data");
    expect_that!(data, some(anything()));
    expect_that!(data.map(|data| descendants(data, "previousValue")), some(is_empty()));

    teardown(&broker::front()?, &id, &building).await;
    Ok(())
}
