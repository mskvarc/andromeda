//! ETSI `046_41`: adding an attribute notifies a subscription with `showChanges` and the `attributeCreated` trigger
//! without any previous value (clause 5.8.6).

use crate::{
    context_information::subscription::subscription_notification_behaviour::{
        change_case::{ContextForm, EntityChange, change_notification, show_changes_subscription},
        notification_case::{descendants, teardown},
    },
    suite_payloads::bare_building,
};
use andromeda_test_suite::{broker, namespace::Namespace, robot_literal};
use googletest::prelude::*;
use ngsi_ld::{
    model::EntityFragment,
    subscription::{NotificationTrigger, SubscriptionId},
};

/// Pins: Create attribute and check that notification has no previous value
/// Clause: 5.8.6
/// Changed: Wait for notification parses the body as json.loads('''<body>'''), a Python '''...''' literal, so a backslash
/// in the body is read as an escape and a ''' or a final ' ends the literal; the port refuses all three (not contains
/// "\\", not contains "'''", not endsWith "'") and then requires the body to be JSON, which is stricter only where an
/// escape leaves the keyword reading, and accepting, a body other than the one sent
#[gtest]
#[tokio::test]
async fn e046_41_01_create_and_check_no_notification_is_sent() -> Result<()> {
    let namespace = Namespace::fresh();
    let id: SubscriptionId = namespace.urn_text("Subscription", 1).parse()?;
    let building = bare_building(&namespace, "046_41")?;
    let fragment = EntityFragment::builder().property("name", "Tour Eiffel").build()?;
    let (notification, _subscription) = change_notification(
        &namespace,
        &["Building", "name"],
        &[&building],
        ContextForm::List,
        |uri| show_changes_subscription(&id, uri, Some(NotificationTrigger::AttributeCreated)),
        EntityChange::UpdateAttributes(&fragment, ContextForm::List),
    )
    .await?;
    let body = robot_literal::read(&notification.body)?;
    let data = body.get("data");
    expect_that!(data, some(anything()));
    expect_that!(data.map(|data| descendants(data, "previousValue")), some(is_empty()));

    teardown(&broker::front()?, &id, &building).await;
    Ok(())
}
