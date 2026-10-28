//! ETSI `046_05`: a notification of a watched attribute's change includes the subscribed entities that changed and satisfy
//! the subscription's `q` (clauses 5.8.6, 4.9 and 4.10).

use crate::{
    context_information::subscription::subscription_notification_behaviour::notification_case::{air_quality_updated, created, subscribed, teardown},
    suite_payloads::building_simple_attributes,
};
use andromeda_test_suite::{broker, namespace::Namespace, notification_sink::NotificationSink, robot_literal, vocabulary::Vocabulary};
use googletest::prelude::*;
use ngsi_ld::{
    http::ContextDelivery,
    model::NgsiLdMediaType,
    query::{EntitySelector, q::Query},
    subscription::{Activation, Endpoint, NotificationParams, Subscription, SubscriptionId},
};
use serde_json::{Value, json};

/// Pins: The notification message shall include all the subscribed Entities that changed and that match (as mandated by
/// clauses 4.9 and 4.10) the query and geoquery conditions
/// Clause: 5.8.6
/// Corrected: fix(`046_05`): body q spells the space literally, not percent-encoded (4.9)
/// Changed: Wait for notification parses the body as json.loads('''<body>'''), a Python '''...''' literal, so a backslash
/// in the body is read as an escape and a ''' or a final ' ends the literal; the port refuses all three (not contains
/// "\\", not contains "'''", not endsWith "'") and then requires the body to be JSON, which is stricter only where an
/// escape leaves the keyword reading, and accepting, a body other than the one sent
/// Changed: The non-empty `$.data` is required to be an array, which the normalized notification always carries; the Hurl
/// line also accepts an object or a string
#[gtest]
#[tokio::test]
async fn e046_05_01_check_that_a_notification_is_sent_with_all_entities() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Building", "name", "subCategory", "airQualityLevel", "almostFull"]).await?;
    let sink = NotificationSink::start().await?;
    let as_string = broker.with_context(vocabulary.context())?.with_context_delivery(ContextDelivery::LdJsonBody);
    let as_list = broker
        .with_context(vocabulary.context_list())?
        .with_context_delivery(ContextDelivery::LdJsonBody);
    let building = building_simple_attributes(&namespace, 1)?;
    created(&as_string, &building).await?;

    let id: SubscriptionId = namespace.urn_text("Subscription", 1).parse()?;
    let endpoint = Endpoint::builder(sink.endpoint(1)?.as_str()).accept(NgsiLdMediaType::Json).build()?;
    // The subscription owns its identifier and keeps it private; the teardown deletes it by the same one.
    let subscription = Subscription::builder()
        .id(id.clone())
        .activation(Activation::Active)
        .watched_attributes(["airQualityLevel"])
        .q("name==\"Eiffel Tower\"".parse::<Query>()?)
        .entities([EntitySelector::new("Building".parse()?).with_ids(building.id.clone())])
        .notification(NotificationParams::builder(endpoint).build()?)
        .build()?;
    let _subscription = subscribed(&as_list, &subscription).await?;

    air_quality_updated(&as_list, &building).await?;
    let notification = sink.wait_for(1, 1).await?;
    let body = robot_literal::read(&notification.body)?;
    expect_that!(body.pointer("/subscriptionId"), some(eq(&json!(id.to_string()))));
    expect_that!(body.pointer("/data").and_then(Value::as_array).map(Vec::len), some(gt(0)));
    expect_that!(body.pointer("/data/0/id"), some(eq(&json!(building.id.to_string()))));
    expect_that!(
        body.pointer("/data/0/airQualityLevel/value"),
        some(any!(eq(&json!(5)), eq(&json!("5")), eq(&json!("5.0"))))
    );
    expect_that!(body.pointer("/data/0/name/value"), some(eq(&json!("Eiffel Tower"))));

    teardown(&broker, &id, &building).await;
    Ok(())
}
