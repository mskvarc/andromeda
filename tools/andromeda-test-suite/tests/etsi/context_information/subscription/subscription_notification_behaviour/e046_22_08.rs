//! Deleting every instance of a watched attribute notifies a normalized subscription with the `attributeDeleted`
//! trigger, each instance reported as the NGSI-LD Null with its `datasetId` (clauses 5.8.6 and 4.5.5).

use crate::context_information::subscription::subscription_notification_behaviour::{
    deletion_case::{NGSI_LD_NULL, NotifiedAs, named_building_deletion_notification},
    notification_case::teardown,
};
use andromeda_test_suite::{broker, comparison::matches_document, namespace::Namespace};
use googletest::prelude::*;
use ngsi_ld::{http::operation::InstanceSelection, subscription::SubscriptionId};
use serde_json::{Value, json};

/// Pins: Delete all instances of an attribute and check the received notification
/// Clause: 5.8.6, 4.5.5
/// Expected: every deleted instance is nullified with its datasetId (clause 5.8.6)
#[gtest]
#[tokio::test]
async fn e046_22_08_check_that_a_notification_is_sent_with_matching_entity() -> Result<()> {
    let namespace = Namespace::fresh();
    let id: SubscriptionId = namespace.urn_text("Subscription", 1).parse()?;
    let (notification, building, _subscription) = named_building_deletion_notification(&namespace, &id, NotifiedAs::Normalized, InstanceSelection::All).await?;
    let body = notification.json()?;
    expect_that!(body.pointer("/subscriptionId"), some(eq(&json!(id.to_string()))));
    expect_that!(body.pointer("/data").and_then(Value::as_array).map(Vec::len), some(eq(1)));
    expect_that!(body.pointer("/data/0/id"), some(eq(&json!(building.id.to_string()))));
    expect_that!(
        body.pointer("/data/0"),
        some(
            matches_document(json!({
                "type": "Building",
                "name": [
                    {"type": "Property", "value": NGSI_LD_NULL.as_str()},
                    {
                        "type": "Property",
                        "datasetId": namespace.urn_text("Dataset", "french-name"),
                        "value": NGSI_LD_NULL.as_str(),
                    },
                    {
                        "type": "Property",
                        "datasetId": namespace.urn_text("Dataset", "german-name"),
                        "value": NGSI_LD_NULL.as_str(),
                    },
                ],
                "locatedAt": [
                    {"type": "Relationship", "object": namespace.urn_text("City", "Paris")},
                    {
                        "type": "Relationship",
                        "datasetId": namespace.urn_text("Dataset", "french-name"),
                        "object": namespace.urn_text("Ville", "Paris"),
                    },
                    {
                        "type": "Relationship",
                        "datasetId": namespace.urn_text("Dataset", "spanish-name"),
                        "object": namespace.urn_text("Ciudad", "París"),
                    },
                ],
            }))
            .hiding_everywhere("id")
        )
    );

    teardown(&broker::front()?, &id, &building).await;
    Ok(())
}
