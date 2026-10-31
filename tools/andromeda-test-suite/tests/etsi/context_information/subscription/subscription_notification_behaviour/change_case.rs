//! The steps the notification tests share whose notification endpoint starts serving only after their subscription
//! exists: the entities they create, the subscription, the one change that triggers the notification, and the wait
//! for it.

use crate::{
    context_information::subscription::subscription_notification_behaviour::notification_case::{attributes_updated, subscription_created},
    entity_operations::{create_entity, create_entity_with_context_string, ld_json_body_client},
};
use andromeda_test_suite::{
    broker::{self, BrokerClient},
    cleanup::Cleanup,
    namespace::Namespace,
    notification_sink::{NotificationSink, ReceivedNotification},
    problem::answered,
    teardown::AnyAnswer,
    vocabulary::Vocabulary,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{ContextDelivery, operation::InstanceSelection},
    model::{AttributeName, Entity, EntityFragment, EntityId, Input},
    query::EntitySelector,
    subscription::{ChangeReporting, Endpoint, NotificationParams, NotificationTrigger, Subscription, SubscriptionId},
};
use serde_json::{Value, json};
use url::Url;

/// How the body that creates a case's entities names the vocabulary's `@context`.
#[derive(Debug, Clone, Copy)]
pub(crate) enum ContextForm {
    /// A plain string.
    String,
    /// A one-element list.
    List,
}

/// The change a case makes to its entity once the sink serves, each requiring 204 unless it says otherwise.
#[derive(Debug, Clone, Copy)]
pub(crate) enum EntityChange<'a> {
    /// `Update Attributes` with the fragment: `PATCH entities/<id>/attrs/`, the `@context` in the form named.
    UpdateAttributes(&'a EntityFragment, ContextForm),
    /// `Delete Attribute` of the name: `DELETE entities/<id>/attrs/<name>?deleteAll=false` as `application/json`, the
    /// `@context` in the `Link` header.
    DeleteAttribute(&'a AttributeName),
    /// `Delete Entity`, then `Delete Temporal Entity` whatever its answer.
    DeleteEntity,
    /// `Create Entity` of the entity, the `@context` in the form the case creates its entities with, requiring 201.
    CreateEntity(&'a Entity<Input>),
}

/// With the sink not serving yet, registers `terms` and creates `entities` in that order with the `@context` as
/// `created_with` says, then the subscription `subscription` builds from the sink's endpoint URL (each requiring 201),
/// then starts the sink, makes `change` (to the first of `entities` when it changes one) and returns the first
/// notification the sink receives.
pub(crate) async fn change_notification(
    namespace: &Namespace,
    terms: &[&str],
    entities: &[&Entity<Input>],
    created_with: ContextForm,
    subscription: impl FnOnce(&Url) -> Result<Subscription<Input>>,
    change: EntityChange<'_>,
) -> Result<(ReceivedNotification, Cleanup)> {
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, namespace, terms).await?;
    let sink = NotificationSink::refusing()?;
    for entity in entities {
        created(&broker, &vocabulary, entity, created_with).await?;
    }
    let as_list = ld_json_body_client(&broker, &vocabulary)?;
    let subscribed = subscription_created(&as_list, &subscription(&sink.endpoint(1)?)?).await?;
    let sink = sink.start_serving().await?;

    let changed = || -> Result<&EntityId> { Ok(&entities.first().or_fail()?.id) };
    match change {
        EntityChange::UpdateAttributes(fragment, form) => {
            let client = match form {
                ContextForm::String => broker.with_context(vocabulary.context())?.with_context_delivery(ContextDelivery::LdJsonBody),
                ContextForm::List => ld_json_body_client(&broker, &vocabulary)?,
            };
            let updated = attributes_updated(&client, changed()?, fragment).await;
            verify_that!(&updated, answered(StatusCode::NO_CONTENT))?;
        }
        EntityChange::DeleteAttribute(name) => {
            let as_link = broker.with_context(vocabulary.context())?;
            let answer = as_link
                .with_responses()
                .entities()
                .delete_attribute(changed()?, name)
                .instances(InstanceSelection::Selected(None))
                .send()
                .await;
            verify_that!(&answer, answered(StatusCode::NO_CONTENT))?;
        }
        EntityChange::DeleteEntity => {
            broker.entities().delete(changed()?).send().await?;
            broker.temporal().delete(changed()?).send().await.any_answer();
        }
        EntityChange::CreateEntity(entity) => created(&broker, &vocabulary, entity, created_with).await?,
    }
    Ok((sink.wait_for(1, 1).await?, subscribed))
}

/// Creates `entity` with the `@context` of `vocabulary` in the form `form` names, requiring 201.
async fn created(broker: &BrokerClient, vocabulary: &Vocabulary, entity: &Entity<Input>, form: ContextForm) -> Result<()> {
    match form {
        ContextForm::String => create_entity_with_context_string(broker, vocabulary, entity).await,
        ContextForm::List => create_entity(broker, vocabulary, entity).await,
    }
}

/// The Building `building_located_street_geo` builds, as a normalized notification carries it without its `id`.
pub(crate) fn street_building_document(namespace: &Namespace) -> Value {
    json!({
        "type": "Building",
        "location": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [-8.5, 41.2]}},
        "street": {"type": "LanguageProperty", "languageMap": {"fr": "Grand Place", "nl": "Grote Markt"}},
        "locatedAt": {"type": "Relationship", "object": namespace.urn_text("City", "Paris")},
        "name": {"type": "Property", "value": "Eiffel Tower"},
    })
}

/// The subscription `id` to every Building notifying `uri` with `showChanges` set, with the one trigger `trigger` names
/// or the default ones.
pub(crate) fn show_changes_subscription(id: &SubscriptionId, uri: &Url, trigger: Option<NotificationTrigger>) -> Result<Subscription<Input>> {
    let endpoint = Endpoint::builder(uri.as_str()).build()?;
    // The subscription owns its identifier and keeps it private; the teardown deletes it by the same one.
    let subscription = Subscription::builder()
        .id(id.clone())
        .entities([EntitySelector::new("Building".parse()?)])
        .notification(NotificationParams::builder(endpoint).show_changes(ChangeReporting::WithPrevious).build()?);
    Ok(match trigger {
        Some(trigger) => subscription.notification_triggers([trigger]).build()?,
        None => subscription.build()?,
    })
}
