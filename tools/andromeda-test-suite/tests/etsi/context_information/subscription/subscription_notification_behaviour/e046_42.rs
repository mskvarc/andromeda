//! A subscription whose notification picks `id` and `name` notifies only those members of the updated entity (clauses
//! 5.8.6 and 4.21).

use crate::{
    building_payloads::{BUILDING_LOCATED_STREET_TERMS, building_located_street_geo},
    context_information::subscription::subscription_notification_behaviour::{
        change_case::{ContextForm, EntityChange, change_notification},
        notification_case::{data_item, teardown},
    },
};
use andromeda_test_suite::{broker, comparison::matches_document, namespace::Namespace};
use googletest::prelude::*;
use ngsi_ld::{
    model::{EntityFragment, NgsiLdMediaType},
    query::{EntitySelector, projection::Projection},
    subscription::{Endpoint, NotificationParams, Subscription, SubscriptionId},
};
use serde_json::{Value, json};

/// Pins: Update an attribute and check the notification contains entity members specified in the pick parameter
/// Clause: 5.8.6, 4.21
#[gtest]
#[tokio::test]
async fn e046_42_01_update_attribute_and_check_entity_members_in_notification() -> Result<()> {
    let namespace = Namespace::fresh();
    let id: SubscriptionId = namespace.urn_text("Subscription", 1).parse()?;
    let building = building_located_street_geo(&namespace, namespace.urn("Building", "046_42")?)?;
    let fragment = EntityFragment::builder().property("name", "Tour Eiffel").build()?;
    let subscription = |uri: &url::Url| -> Result<Subscription<_>> {
        let endpoint = Endpoint::builder(uri.as_str()).accept(NgsiLdMediaType::Json).build()?;
        // The subscription owns its identifier and keeps it private; the teardown deletes it by the same one.
        Ok(Subscription::builder()
            .id(id.clone())
            .entities([EntitySelector::new("Building".parse()?)])
            .notification(NotificationParams::builder(endpoint).pick("id,name".parse::<Projection>()?).build()?)
            .build()?)
    };
    let (notification, _subscription) = change_notification(
        &namespace,
        &BUILDING_LOCATED_STREET_TERMS,
        &[&building],
        ContextForm::String,
        subscription,
        EntityChange::UpdateAttributes(&fragment, ContextForm::List),
    )
    .await?;
    let body = notification.json()?;
    expect_that!(body.pointer("/subscriptionId"), some(eq(&json!(id.to_string()))));
    expect_that!(body.pointer("/data").and_then(Value::as_array).map(Vec::len), some(eq(1)));
    let expected = json!({"name": {"type": "Property", "value": "Tour Eiffel"}});
    expect_that!(data_item(&body, &building.id), some(matches_document(expected).hiding_everywhere("id")));

    teardown(&broker::front()?, &id, &building).await;
    Ok(())
}
