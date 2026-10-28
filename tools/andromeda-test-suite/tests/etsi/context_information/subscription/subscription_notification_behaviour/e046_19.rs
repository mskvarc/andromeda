//! ETSI `046_19`: a subscription's `datasetId` keeps in the notified entity only the attribute instances of that dataset
//! (clauses 4.5.5 and 5.8.6).

use crate::{
    context_information::subscription::subscription_notification_behaviour::notification_case::{
        attributes_updated,
        created,
        french_name_update,
        subscription_created,
        teardown,
    },
    suite_payloads::{building_named_in_datasets, dataset},
};
use andromeda_test_suite::{
    broker,
    comparison::matches_document,
    namespace::Namespace,
    notification_sink::NotificationSink,
    robot_literal,
    teardown::AnyAnswer,
    vocabulary::Vocabulary,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::ContextDelivery,
    model::{NgsiLdMediaType, Representation},
    query::EntitySelector,
    subscription::{Endpoint, NotificationParams, Subscription, SubscriptionId},
};
use serde_json::json;

/// Pins: If a subscription has a datasetId member instances should be filtered based on that datasetId.
/// Clause: 4.5.5, 5.8.6
/// Changed: Wait for notification parses the body as json.loads('''<body>'''), a Python '''...''' literal, so a backslash
/// in the body is read as an escape and a ''' or a final ' ends the literal; the port refuses all three (not contains
/// "\\", not contains "'''", not endsWith "'") and then requires the body to be JSON, which is stricter only where an
/// escape leaves the keyword reading, and accepting, a body other than the one sent
#[gtest]
#[tokio::test]
async fn e046_19_check_that_only_the_attribute_instances_that_match_the_dataset_id_member_are_included_in_the_entity_in_the_notification() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Building", "name", "locatedAt"]).await?;
    let sink = NotificationSink::start().await?;
    let as_string = broker.with_context(vocabulary.context())?.with_context_delivery(ContextDelivery::LdJsonBody);
    let as_list = broker
        .with_context(vocabulary.context_list())?
        .with_context_delivery(ContextDelivery::LdJsonBody);
    let building = building_named_in_datasets(&namespace, 1)?;
    created(&as_string, &building).await?;

    let id: SubscriptionId = namespace.urn_text("Subscription", 1).parse()?;
    let endpoint = Endpoint::builder(sink.endpoint(1)?.as_str()).accept(NgsiLdMediaType::JsonLd).build()?;
    // The subscription owns its identifier and keeps it private; the teardown deletes it by the same one.
    let subscription = Subscription::builder()
        .id(id.clone())
        .entities([EntitySelector::new("Building".parse()?).with_ids(building.id.clone())])
        .notification(NotificationParams::builder(endpoint).format(Representation::Normalized).build()?)
        .dataset_ids([dataset(&namespace, "french-name")?])
        .build()?;
    let _subscription = subscription_created(&as_list, &subscription).await?;

    attributes_updated(&as_list, &building.id, &french_name_update(&namespace)?).await.any_answer();

    let notification = sink.wait_for(1, 1).await?;
    let body = robot_literal::read(&notification.body)?;
    expect_that!(body.pointer("/subscriptionId"), some(eq(&json!(id.to_string()))));
    expect_that!(body.pointer("/data/0/id"), some(eq(&json!(building.id.to_string()))));
    expect_that!(
        body.pointer("/data/0"),
        some(
            matches_document(json!({
                "type": "Building",
                "name": {"type": "Property", "value": "Le Tour Eiffel", "datasetId": namespace.urn_text("Dataset", "french-name")},
                "locatedAt": {
                    "type": "Relationship",
                    "datasetId": namespace.urn_text("Dataset", "french-name"),
                    "object": namespace.urn_text("Ville", "Paris"),
                },
                "@context": vocabulary.url().as_str(),
            }))
            .hiding_everywhere("id")
        )
    );

    teardown(&broker, &id, &building).await;
    Ok(())
}
