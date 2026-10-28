//! ETSI `046_40`: creating an entity notifies a subscription with `showChanges` and the `entityCreated` trigger
//! without any previous value (clause 5.8.6).

use crate::{
    context_information::subscription::subscription_notification_behaviour::{
        change_case::{ContextForm, EntityChange, change_notification, show_changes_subscription},
        notification_case::{descendants, teardown},
    },
    suite_payloads::{BUILDING_SIMPLE_TERMS, building_simple_attributes},
};
use andromeda_test_suite::{broker, namespace::Namespace, robot_literal};
use googletest::prelude::*;
use ngsi_ld::subscription::{NotificationTrigger, SubscriptionId};

/// Pins: Create entity and check that notification has no previous value
/// Clause: 5.8.6
/// Changed: Wait for notification parses the body as json.loads('''<body>'''), a Python '''...''' literal, so a backslash
/// in the body is read as an escape and a ''' or a final ' ends the literal; the port refuses all three (not contains
/// "\\", not contains "'''", not endsWith "'") and then requires the body to be JSON, which is stricter only where an
/// escape leaves the keyword reading, and accepting, a body other than the one sent
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
    let body = robot_literal::read(&notification.body)?;
    let data = body.get("data");
    expect_that!(data, some(anything()));
    expect_that!(data.map(|data| descendants(data, "previousValue")), some(is_empty()));

    teardown(&broker::front()?, &id, &building).await;
    Ok(())
}
