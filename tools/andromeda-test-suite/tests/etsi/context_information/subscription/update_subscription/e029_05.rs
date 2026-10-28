//! ETSI `029_05`: an update that names an entity type by its term expands the term to its IRI through the `@context`
//! the update carries, and the stored subscription keeps the expansion (clause 5.8.2).

use crate::{
    suite_keywords::{create_timed_building_subscription, ld_json_body_client, subscription_updated, update_subscription_request},
    suite_payloads::key_values_notification,
};
use andromeda_test_suite::{
    broker::{self, BrokerClient},
    comparison::matches_document,
    namespace::Namespace,
    notification_sink::NotificationSink,
    teardown::AnyAnswer,
    vocabulary::Vocabulary,
    wire::json_body,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{UncheckedDocument, UncheckedRequest, headers::Accept},
    model::NgsiLdMediaType,
    query::EntitySelector,
    subscription::{SubscriptionFragment, SubscriptionId},
};
use serde_json::json;
use url::Url;

/// `Update Subscription` of `id` that names the entity type `Vehicle` and a keyValues notification to `uri`, sent by
/// `client`.
fn update_to_vehicle(client: &BrokerClient, id: &SubscriptionId, uri: &Url) -> Result<UncheckedRequest> {
    let fragment = SubscriptionFragment::builder().entities([EntitySelector::new("Vehicle".parse()?)]).build()?;
    let document = UncheckedDocument::try_from(fragment.render())?.set_member("notification", &key_values_notification(uri)?);
    update_subscription_request(client, id, &document)
}

/// Pins: Check that one can update a subcription: Term to URI expansion of Attribute names shall be observed
/// Clause: 5.8.2
#[gtest]
#[tokio::test]
async fn e029_05_01_update_subscription_with_term_to_uri_expansion_with_context() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Building", "Vehicle"]).await?;
    let sink = NotificationSink::start().await?;
    let id: SubscriptionId = namespace.urn_text("Subscription", 1).parse()?;
    let endpoint_url = sink.endpoint(1)?;
    let client = ld_json_body_client(&broker, &vocabulary)?;
    let _subscription = create_timed_building_subscription(&client, &id, &endpoint_url).await?;

    let linked = broker.with_context(vocabulary.context())?;
    subscription_updated(&linked, update_to_vehicle(&linked, &id, &endpoint_url)?).await?;

    let received = linked
        .with_accept(NgsiLdMediaType::JsonLd)
        .with_responses()
        .subscriptions()
        .retrieve(&id)
        .send()
        .await?;
    let body = json_body(&received)?;
    expect_that!(
        &body,
        matches_document(json!({
            "id": id.to_string(),
            "type": "Subscription",
            "timeInterval": 5,
            "entities": [{"type": "Vehicle"}],
            "notification": {
                "format": "keyValues",
                "endpoint": {"uri": endpoint_url.as_str(), "accept": "application/json"}
            },
            "status": "active",
        }))
        .hiding_everywhere("@context")
        .hiding_everywhere("jsonldContext")
        .hiding_everywhere("timesFailed")
        .hiding_everywhere("timesSent")
        .hiding_everywhere("notificationTrigger")
    );

    broker.subscriptions().delete(&id).send().await.any_answer();
    Ok(())
}

/// Pins: Check that one can update a subcription: Term to URI expansion of Attribute names shall be observed
/// Clause: 5.8.2
#[gtest]
#[tokio::test]
async fn e029_05_02_update_subscription_with_term_to_uri_expansion_without_context() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Building", "Vehicle"]).await?;
    let sink = NotificationSink::start().await?;
    let id: SubscriptionId = namespace.urn_text("Subscription", 1).parse()?;
    let endpoint_url = sink.endpoint(1)?;
    let client = ld_json_body_client(&broker, &vocabulary)?;
    let _subscription = create_timed_building_subscription(&client, &id, &endpoint_url).await?;

    let linked = broker.with_context(vocabulary.context())?;
    subscription_updated(&linked, update_to_vehicle(&linked, &id, &endpoint_url)?).await?;

    let received = broker.with_accept(Accept::any()).with_responses().subscriptions().retrieve(&id).send().await?;
    let body = json_body(&received)?;
    let vehicle = namespace.expanded_term("Vehicle")?;
    expect_that!(
        &body,
        matches_document(json!({
            "id": id.to_string(),
            "type": "Subscription",
            "timeInterval": 5,
            "entities": [{"type": vehicle}],
            "notification": {
                "format": "keyValues",
                "endpoint": {"uri": endpoint_url.as_str(), "accept": "application/json"}
            },
            "status": "active",
        }))
        .hiding_everywhere("@context")
        .hiding_everywhere("jsonldContext")
        .hiding_everywhere("timesFailed")
        .hiding_everywhere("timesSent")
        .hiding_everywhere("notificationTrigger")
    );

    broker.subscriptions().delete(&id).send().await.any_answer();
    Ok(())
}
