//! The steps the linked entity notification cases of the ETSI `046_23` to `046_30` Robot runs share: a Building
//! located at the City Paris, a subscription that joins the City into its notifications, and the `name` update that
//! triggers one.

use crate::{
    context_information::subscription::subscription_notification_behaviour::change_case::{ContextForm, EntityChange, change_notification},
    suite_keywords::delete_entities,
};
use andromeda_test_suite::{broker::BrokerClient, cleanup::Cleanup, namespace::Namespace, notification_sink::ReceivedNotification, teardown::AnyAnswer};
use googletest::prelude::*;
use ngsi_ld::{
    model::{Entity, EntityFragment, EntityId, Input, Relationship},
    query::EntitySelector,
    subscription::{Endpoint, NotificationParams, Subscription, SubscriptionId},
};
use url::Url;

/// The Building `urn:ngsi-ld:Building:<case>:<ns>-EiffelTower` of the case `case`, with a `locatedAt` Relationship
/// to the City `urn:ngsi-ld:City:<ns>-Paris`.
pub(crate) fn eiffel_tower(namespace: &Namespace, case: &str) -> Result<Entity<Input>> {
    Ok(Entity::builder()
        .id(format!("urn:ngsi-ld:Building:{case}:{namespace}-EiffelTower").parse::<EntityId>()?)
        .entity_type("Building")
        .relationship("locatedAt", Relationship::builder(namespace.urn("City", "Paris")?))
        .build()?)
}

/// With the sink not serving yet, registers `terms`, creates `building` and `city` and the subscription `id` to every
/// Building notifying the sink with the parameters `notification` builds from its endpoint (each requiring 201), then
/// starts the sink, adds the `name` Tour Eiffel to `building` (requiring 204) and returns the first notification the
/// sink receives.
pub(crate) async fn linked_notification(
    namespace: &Namespace,
    terms: &[&str],
    building: &Entity<Input>,
    city: &Entity<Input>,
    id: &SubscriptionId,
    notification: impl FnOnce(Endpoint) -> Result<NotificationParams<Input>>,
) -> Result<(ReceivedNotification, Cleanup)> {
    let subscription = |uri: &Url| -> Result<Subscription<Input>> {
        // The subscription owns its identifier and keeps it private; the teardown deletes it by the same one.
        Ok(Subscription::builder()
            .id(id.clone())
            .entities([EntitySelector::new("Building".parse()?)])
            .notification(notification(Endpoint::builder(uri.as_str()).build()?)?)
            .build()?)
    };
    let fragment = EntityFragment::builder().property("name", "Tour Eiffel").build()?;
    let change = EntityChange::UpdateAttributes(&fragment, ContextForm::List);
    change_notification(namespace, terms, &[building, city], ContextForm::List, subscription, change).await
}

/// Deletes the subscription `id`, the Building `building` and the City Paris the case created, whatever the answers.
pub(crate) async fn linked_teardown(broker: &BrokerClient, namespace: &Namespace, id: &SubscriptionId, building: &Entity<Input>) -> Result<()> {
    broker.subscriptions().delete(id).send().await.any_answer();
    delete_entities(broker, &[&building.id, &namespace.urn("City", "Paris")?]).await;
    Ok(())
}
