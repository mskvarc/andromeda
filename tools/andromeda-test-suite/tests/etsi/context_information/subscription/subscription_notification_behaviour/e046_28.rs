//! ETSI `046_28`: a subscription with an `inline` join and `sysAttrs` notifies the Building and the City inside it with
//! their system timestamps (clauses 4.5.23 and 5.8.6).

use crate::{
    context_information::subscription::subscription_notification_behaviour::{
        linked_case::{eiffel_tower, linked_notification, linked_teardown},
        notification_case::{ANY_SYSTEM_TIME, data_item},
    },
    suite_payloads::city_paris,
};
use andromeda_test_suite::{broker, comparison::matches_document, namespace::Namespace, robot_literal};
use googletest::prelude::*;
use ngsi_ld::{
    query::Join,
    subscription::{NotificationParams, SubscriptionId, SystemAttributeReporting},
};
use serde_json::{Value, json};

/// Pins: Create an attribute in linking entity and check the notification contains linked entity with sysAttrs
/// Clause: 4.5.23, 5.8.6
/// Changed: Wait for notification parses the body as json.loads('''<body>'''), a Python '''...''' literal, so a backslash
/// in the body is read as an escape and a ''' or a final ' ends the literal; the port refuses all three (not contains
/// "\\", not contains "'''", not endsWith "'") and then requires the body to be JSON, which is stricter only where an
/// escape leaves the keyword reading, and accepting, a body other than the one sent
#[gtest]
#[tokio::test]
async fn e046_28_01_check_that_a_notification_is_sent_with_linked_entity_in_inline_representation_with_sys_attrs() -> Result<()> {
    let namespace = Namespace::fresh();
    let id: SubscriptionId = namespace.urn_text("Subscription", 1).parse()?;
    let building = eiffel_tower(&namespace, "046_28")?;
    let (notification, _subscription) = linked_notification(
        &namespace,
        &["Building", "locatedAt", "City", "name"],
        &building,
        &city_paris(&namespace)?,
        &id,
        |endpoint| {
            Ok(NotificationParams::builder(endpoint)
                .join(Join::Inline)
                .join_level(1)
                .system_attributes(SystemAttributeReporting::Include)
                .build()?)
        },
    )
    .await?;
    let body = robot_literal::read(&notification.body)?;
    let paris = namespace.urn("City", "Paris")?;
    expect_that!(body.pointer("/subscriptionId"), some(eq(&json!(id.to_string()))));
    expect_that!(body.pointer("/data").and_then(Value::as_array).map(Vec::len), some(eq(1)));
    expect_that!(
        data_item(&body, &building.id),
        some(
            matches_document(json!({
                "type": "Building",
                "createdAt": ANY_SYSTEM_TIME,
                "modifiedAt": ANY_SYSTEM_TIME,
                "name": {
                    "type": "Property",
                    "value": "Tour Eiffel",
                    "createdAt": ANY_SYSTEM_TIME,
                    "modifiedAt": ANY_SYSTEM_TIME,
                },
                "locatedAt": {
                    "type": "Relationship",
                    "createdAt": ANY_SYSTEM_TIME,
                    "object": paris.to_string(),
                    "modifiedAt": ANY_SYSTEM_TIME,
                    "entity": {"type": "City", "createdAt": ANY_SYSTEM_TIME, "modifiedAt": ANY_SYSTEM_TIME},
                },
            }))
            .hiding_everywhere("id")
        )
    );

    linked_teardown(&broker::front()?, &namespace, &id, &building).await
}
