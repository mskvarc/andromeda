//! ETSI `046_22_08`: deleting every instance of a watched attribute notifies a normalized subscription with the
//! `attributeDeleted` trigger, each instance reported as the NGSI-LD Null with its `datasetId` (clauses 5.8.6 and
//! 4.5.5).

use crate::context_information::subscription::subscription_notification_behaviour::{
    deletion_case::{NGSI_LD_NULL, NotifiedAs, named_building_deletion_notification},
    notification_case::teardown,
};
use andromeda_test_suite::{broker, comparison::matches_document, namespace::Namespace, robot_literal};
use googletest::prelude::*;
use ngsi_ld::{http::operation::InstanceSelection, subscription::SubscriptionId};
use serde_json::{Value, json};

/// Pins: Delete all instances of an attribute and check the received notification
/// Clause: 5.8.6, 4.5.5
/// Corrected: fix(`046_22_08`): expect every deleted instance nullified with its datasetId (5.8.6)
/// Changed: Wait for notification parses the body as json.loads('''<body>'''), a Python '''...''' literal, so a backslash
/// in the body is read as an escape and a ''' or a final ' ends the literal; the port refuses all three (not contains
/// "\\", not contains "'''", not endsWith "'") and then requires the body to be JSON, which is stricter only where an
/// escape leaves the keyword reading, and accepting, a body other than the one sent
/// Changed: Robot turns the notification body bytes into text as Latin-1 (HttpCtrl hands them over as bytes, which Robot
/// pastes into the json.loads expression), so the expectation spells the UTF-8 bytes of a non-ASCII character as Latin-1
/// characters (ParÃ­s for París); Hurl reads the body as UTF-8, so the port compares with the text those bytes encode,
/// which holds exactly when the keyword's comparison does, the body passing the guards above
/// Changed: The `length` check requires `$.data` to be an array, which a notification always carries; the Hurl line also
/// accepts a string or an object of that length
#[gtest]
#[tokio::test]
async fn e046_22_08_check_that_a_notification_is_sent_with_matching_entity() -> Result<()> {
    let namespace = Namespace::fresh();
    let id: SubscriptionId = namespace.urn_text("Subscription", 1).parse()?;
    let (notification, building, _subscription) = named_building_deletion_notification(&namespace, &id, NotifiedAs::Normalized, InstanceSelection::All).await?;
    let body = robot_literal::read(&notification.body)?;
    expect_that!(body.pointer("/subscriptionId"), some(eq(&json!(id.to_string()))));
    expect_that!(body.pointer("/data").and_then(Value::as_array).map(Vec::len), some(eq(1)));
    expect_that!(body.pointer("/data/0/id"), some(eq(&json!(building.id.to_string()))));
    expect_that!(
        body.pointer("/data/0"),
        some(
            matches_document(json!({
                "type": "Building",
                "name": [
                    {"type": "Property", "value": NGSI_LD_NULL},
                    {
                        "type": "Property",
                        "datasetId": namespace.urn_text("Dataset", "french-name"),
                        "value": NGSI_LD_NULL,
                    },
                    {
                        "type": "Property",
                        "datasetId": namespace.urn_text("Dataset", "german-name"),
                        "value": NGSI_LD_NULL,
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
