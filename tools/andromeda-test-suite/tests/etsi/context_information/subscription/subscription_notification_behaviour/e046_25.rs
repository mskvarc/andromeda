//! A simplified subscription with a `flat` join notifies the Building and the City it links to in the simplified form
//! (clauses 4.5.23 and 5.8.6).

use crate::{
    context_information::subscription::subscription_notification_behaviour::{
        linked_case::{eiffel_tower, linked_notification, linked_teardown},
        notification_case::data_item,
    },
    linked_place_payloads::city_paris_in_france,
};
use andromeda_test_suite::{broker, comparison::matches_document, namespace::Namespace};
use googletest::prelude::*;
use ngsi_ld::{
    model::Representation,
    query::Join,
    subscription::{NotificationParams, SubscriptionId},
};
use serde_json::{Value, json};

/// Pins: Create an attribute in linking entity and check the notification contains linked entity in simplified format
/// Clause: 4.5.23, 5.8.6
#[gtest]
#[tokio::test]
async fn e046_25_01_check_that_a_notification_is_sent_with_flat_linked_entity_in_simplified_format() -> Result<()> {
    let namespace = Namespace::fresh();
    let id: SubscriptionId = namespace.urn_text("Subscription", 1).parse()?;
    let building = eiffel_tower(&namespace, "046_25")?;
    let (notification, _subscription) = linked_notification(
        &namespace,
        &["Building", "locatedAt", "City", "name", "isInCountry"],
        &building,
        &city_paris_in_france(&namespace)?,
        &id,
        |endpoint| {
            Ok(NotificationParams::builder(endpoint)
                .join(Join::Flat)
                .join_level(1)
                .format(Representation::Simplified)
                .build()?)
        },
    )
    .await?;
    let body = notification.json()?;
    let paris = namespace.urn("City", "Paris")?;
    expect_that!(body.pointer("/subscriptionId"), some(eq(&json!(id.to_string()))));
    expect_that!(body.pointer("/data").and_then(Value::as_array).map(Vec::len), some(eq(2)));
    expect_that!(
        data_item(&body, &building.id),
        some(matches_document(json!({"type": "Building", "name": "Tour Eiffel", "locatedAt": paris.to_string()})).hiding_everywhere("id"))
    );
    expect_that!(
        data_item(&body, &paris),
        some(
            matches_document(json!({
                "type": "City",
                "description": "Paris is the capital and largest city of France",
                "name": "Paris",
                "isInCountry": namespace.urn_text("Country", "France"),
            }))
            .hiding_everywhere("id")
        )
    );

    linked_teardown(&broker::front()?, &namespace, &id, &building).await
}
