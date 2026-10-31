//! An update of a Building notifies a subscription with a `flat` join of level 1 with the Building and, beside it, the
//! City its `locatedAt` targets (clauses 4.5.23 and 5.8.6).

use crate::{
    context_information::subscription::subscription_notification_behaviour::{
        linked_case::{eiffel_tower, linked_notification, linked_teardown},
        notification_case::data_item,
    },
    linked_place_payloads::city_paris,
};
use andromeda_test_suite::{broker, comparison::matches_document, namespace::Namespace};
use googletest::prelude::*;
use ngsi_ld::{
    query::Join,
    subscription::{NotificationParams, SubscriptionId},
};
use serde_json::{Value, json};

/// Pins: Create an attribute in linking entity and check the notification contains linked entity
/// Clause: 4.5.23, 5.8.6
#[gtest]
#[tokio::test]
async fn e046_23_01_check_that_a_notification_is_sent_with_linked_entity_in_flat_representation() -> Result<()> {
    let namespace = Namespace::fresh();
    let id: SubscriptionId = namespace.urn_text("Subscription", 1).parse()?;
    let building = eiffel_tower(&namespace, "046_23")?;
    let (notification, _subscription) = linked_notification(
        &namespace,
        &["Building", "locatedAt", "City", "name"],
        &building,
        &city_paris(&namespace)?,
        &id,
        |endpoint| Ok(NotificationParams::builder(endpoint).join(Join::Flat).join_level(1).build()?),
    )
    .await?;
    let body = notification.json()?;
    let paris = namespace.urn("City", "Paris")?;
    expect_that!(body.pointer("/subscriptionId"), some(eq(&json!(id.to_string()))));
    expect_that!(body.pointer("/data").and_then(Value::as_array).map(Vec::len), some(eq(2)));
    expect_that!(
        data_item(&body, &building.id),
        some(
            matches_document(json!({
                "type": "Building",
                "name": {"type": "Property", "value": "Tour Eiffel"},
                "locatedAt": {"type": "Relationship", "object": paris.to_string()},
            }))
            .hiding_everywhere("id")
        )
    );
    expect_that!(
        data_item(&body, &paris),
        some(matches_document(json!({"type": "City"})).hiding_everywhere("id"))
    );

    linked_teardown(&broker::front()?, &namespace, &id, &building).await
}
