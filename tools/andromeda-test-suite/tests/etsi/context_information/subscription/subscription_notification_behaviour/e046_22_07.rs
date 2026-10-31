//! Deleting one dataset's instance of a watched attribute notifies a keyValues subscription with the `attributeDeleted`
//! trigger, the instances keyed by dataset and that one reported as the NGSI-LD Null (clauses 5.8.6 and 4.5.5).

use crate::context_information::subscription::subscription_notification_behaviour::{
    deletion_case::{NGSI_LD_NULL, NotifiedAs, named_building_deletion_notification},
    notification_case::teardown,
};
use andromeda_test_suite::{broker, comparison::matches_document, namespace::Namespace};
use googletest::prelude::*;
use ngsi_ld::{http::operation::InstanceSelection, subscription::SubscriptionId};
use serde_json::{Value, json};

/// Pins: Delete an attribute with a specific datasetId and check the received notification
/// Clause: 5.8.6, 4.5.5
#[gtest]
#[tokio::test]
async fn e046_22_07_check_that_a_notification_is_sent_with_matching_entity() -> Result<()> {
    let namespace = Namespace::fresh();
    let id: SubscriptionId = namespace.urn_text("Subscription", 1).parse()?;
    let french_dataset = namespace.dataset_id("Dataset", "french-name")?;
    let selection = InstanceSelection::Dataset(&french_dataset);
    let (notification, building, _subscription) = named_building_deletion_notification(&namespace, &id, NotifiedAs::KeyValues, selection).await?;
    let body = notification.json()?;
    expect_that!(body.pointer("/subscriptionId"), some(eq(&json!(id.to_string()))));
    expect_that!(body.pointer("/data").and_then(Value::as_array).map(Vec::len), some(eq(1)));
    expect_that!(body.pointer("/data/0/id"), some(eq(&json!(building.id.to_string()))));
    let [french, german, spanish] = ["french-name", "german-name", "spanish-name"].map(|name| namespace.urn_text("Dataset", name));
    expect_that!(
        body.pointer("/data/0"),
        some(
            matches_document(json!({
                "type": "Building",
                "name": {"dataset": {
                    german.as_str(): "Eiffelturm",
                    "@none": "Eiffel Tower",
                    french.as_str(): NGSI_LD_NULL.as_str(),
                }},
                "locatedAt": {"dataset": {
                    french.as_str(): namespace.urn_text("Ville", "Paris"),
                    spanish.as_str(): namespace.urn_text("Ciudad", "París"),
                    "@none": namespace.urn_text("City", "Paris"),
                }},
            }))
            .hiding_everywhere("id")
        )
    );

    teardown(&broker::front()?, &id, &building).await;
    Ok(())
}
