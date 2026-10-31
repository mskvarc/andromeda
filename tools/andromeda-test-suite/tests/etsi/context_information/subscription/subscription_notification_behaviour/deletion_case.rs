//! The steps the deletion notification tests share: a subscription created while its endpoint does not serve yet, then
//! the deletion of an entity or of an attribute that notifies it.

use crate::{
    building_payloads::{BUILDING_LOCATED_STREET_TERMS, building_located_street_geo, building_named_in_datasets, eiffel_tower_located},
    context_information::subscription::subscription_notification_behaviour::{
        change_case::street_building_document,
        notification_case::{attributes_updated, subscription_created},
    },
    entity_operations::{create_entity, create_entity_with_context_string, ld_json_body_client, merge_entity, partial_update},
    subscription_operations::{create_subscription_document, simplified_notification},
    subscription_payloads::{key_values_document, key_values_document_reporting},
};
use andromeda_test_suite::{
    broker,
    cleanup::Cleanup,
    namespace::Namespace,
    notification_sink::{NotificationSink, ReceivedNotification},
    problem::answered,
    teardown::AnyAnswer,
    vocabulary::Vocabulary,
};
use googletest::prelude::*;
use http::StatusCode;
use iri_rs::{Iri, iri};
use ngsi_ld::{
    http::{ContextDelivery, operation::InstanceSelection},
    model::{AttributeKind, Change, Entity, EntityFragment, Input, InstanceDeletion, Lossless, NgsiLdMediaType, Representation},
    query::EntitySelector,
    subscription::{Endpoint, NotificationParams, NotificationTrigger, Subscription, SubscriptionId, SystemAttributeReporting},
};
use serde_json::{Value, json};

/// The NGSI-LD Null a deletion writes and a notification reports (clause 4.5.0).
pub(crate) const NGSI_LD_NULL: Iri<&str> = iri!("urn:ngsi-ld:null");

/// With the sink not serving yet, creates the Building `urn:ngsi-ld:Building:<ns>-None` and the keyValues
/// subscription `id` to every Building with the `entityDeleted` trigger and the system attributes `reporting` says
/// (both requiring 201), then starts the sink, deletes the Building (requiring 204) and its temporal evolution, and
/// returns the first notification the sink receives.
pub(crate) async fn entity_deletion_notification(
    namespace: &Namespace,
    id: &SubscriptionId,
    reporting: SystemAttributeReporting,
) -> Result<(ReceivedNotification, Cleanup)> {
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, namespace, &["Building", "name", "subCategory"]).await?;
    let sink = NotificationSink::refusing()?;
    let building = eiffel_tower_located(namespace, "None")?;
    create_entity(&broker, &vocabulary, &building).await?;

    let endpoint_url = sink.endpoint(1)?;
    // The subscription owns its identifier and keeps it private; the teardown deletes it by the same one.
    let subscription = Subscription::builder()
        .id(id.clone())
        .entities([EntitySelector::new("Building".parse()?)])
        .notification(simplified_notification(&endpoint_url)?)
        .notification_triggers([NotificationTrigger::EntityDeleted])
        .build()?;
    let document = key_values_document_reporting(&subscription, &endpoint_url, reporting)?;
    let subscribed = create_subscription_document(&ld_json_body_client(&broker, &vocabulary)?, &subscription, &document).await?;
    let sink = sink.start_serving().await?;

    broker.entities().delete(&building.id).send().await?;
    broker.temporal().delete(&building.id).send().await.any_answer();
    Ok((sink.wait_for(1, 1).await?, subscribed))
}

/// An attribute of the Building [`building_located_street_geo`] builds that a case deletes.
#[derive(Debug, Clone, Copy)]
pub(crate) enum StreetBuildingAttribute {
    /// The `name` Property.
    Name,
    /// The `locatedAt` Relationship.
    LocatedAt,
    /// The `street` `LanguageProperty`.
    Street,
}

impl StreetBuildingAttribute {
    /// The attribute's term.
    const fn term(self) -> &'static str {
        match self {
            StreetBuildingAttribute::Name => "name",
            StreetBuildingAttribute::LocatedAt => "locatedAt",
            StreetBuildingAttribute::Street => "street",
        }
    }

    /// The attribute's kind.
    const fn kind(self) -> AttributeKind {
        match self {
            StreetBuildingAttribute::Name => AttributeKind::Property,
            StreetBuildingAttribute::LocatedAt => AttributeKind::Relationship,
            StreetBuildingAttribute::Street => AttributeKind::LanguageProperty,
        }
    }

    /// The attribute as a normalized notification reports it deleted: the NGSI-LD Null in the shape of its kind.
    fn deleted(self) -> Value {
        match self {
            StreetBuildingAttribute::Name => json!({"type": "Property", "value": NGSI_LD_NULL.as_str()}),
            StreetBuildingAttribute::LocatedAt => json!({"type": "Relationship", "object": NGSI_LD_NULL.as_str()}),
            StreetBuildingAttribute::Street => json!({"type": "LanguageProperty", "languageMap": {"@none": NGSI_LD_NULL.as_str()}}),
        }
    }
}

/// The request a case deletes an attribute with, by writing the NGSI-LD Null.
#[derive(Debug, Clone, Copy)]
pub(crate) enum NullWrite {
    /// `Merge Entity`, `PATCH entities/<id>` with the entity's `id` and `type`, the `@context` a string.
    MergeEntity,
    /// `Update Attributes`, `PATCH entities/<id>/attrs/`, the `@context` a string.
    UpdateAttributes,
    /// `Partial Attribute Update`, `PATCH entities/<id>/attrs/<name>`, the `@context` a one-element list.
    PartialAttributeUpdate,
}

/// With the sink not serving yet, creates the Building `urn:ngsi-ld:Building:<ns>-None` of
/// [`building_located_street_geo`] and the normalized subscription `id` to every Building watching `deleted` with the
/// `attributeDeleted` trigger (both requiring 201), then starts the sink, deletes `deleted` with `write` (requiring 204)
/// and returns the first notification the sink receives, with the Building for the teardown.
pub(crate) async fn street_building_deletion_notification(
    namespace: &Namespace,
    id: &SubscriptionId,
    deleted: StreetBuildingAttribute,
    write: NullWrite,
) -> Result<(ReceivedNotification, Entity<Input>, Cleanup)> {
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, namespace, &BUILDING_LOCATED_STREET_TERMS).await?;
    let sink = NotificationSink::refusing()?;
    let building = building_located_street_geo(namespace, namespace.urn("Building", "None")?)?;
    create_entity_with_context_string(&broker, &vocabulary, &building).await?;

    let endpoint = Endpoint::builder(sink.endpoint(1)?.as_str()).accept(NgsiLdMediaType::Json).build()?;
    // The subscription owns its identifier and keeps it private; the teardown deletes it by the same one.
    let subscription = Subscription::builder()
        .id(id.clone())
        .entities([EntitySelector::new("Building".parse()?)])
        .watched_attributes([deleted.term()])
        .notification(NotificationParams::builder(endpoint).format(Representation::Normalized).build()?)
        .notification_triggers([NotificationTrigger::AttributeDeleted])
        .build()?;
    let as_list = ld_json_body_client(&broker, &vocabulary)?;
    let subscribed = subscription_created(&as_list, &subscription).await?;
    let sink = sink.start_serving().await?;

    let as_string = broker.with_context(vocabulary.context())?.with_context_delivery(ContextDelivery::LdJsonBody);
    match write {
        NullWrite::MergeEntity => {
            let fragment = EntityFragment::for_entity(namespace.urn("Building", "None")?, "Building")
                .delete_kind(deleted.term(), deleted.kind())
                .build()?;
            merge_entity(&as_string, &building.id, &fragment).await?;
        }
        NullWrite::UpdateAttributes => {
            let fragment = EntityFragment::builder().delete_kind(deleted.term(), deleted.kind()).build()?;
            let updated = attributes_updated(&as_string, &building.id, &fragment).await;
            verify_that!(&updated, answered(StatusCode::NO_CONTENT))?;
        }
        NullWrite::PartialAttributeUpdate => {
            let deletion = Change::Delete(InstanceDeletion::new(deleted.kind()));
            partial_update(&as_list, &building.id, &deleted.term().parse()?, deletion, Lossless::Normalized).await?;
        }
    }
    Ok((sink.wait_for(1, 1).await?, building, subscribed))
}

/// The Building of [`building_located_street_geo`] as a normalized notification carries it after `deleted` was
/// deleted, without its `id`.
pub(crate) fn street_building_after_deletion(namespace: &Namespace, deleted: StreetBuildingAttribute) -> Value {
    let mut document = street_building_document(namespace);
    document[deleted.term()] = deleted.deleted();
    document
}

/// The form a subscription notifies the Building [`building_named_in_datasets`] in.
#[derive(Debug, Clone, Copy)]
pub(crate) enum NotifiedAs {
    /// `normalized`.
    Normalized,
    /// `keyValues`, the spelling these tests write.
    KeyValues,
}

/// With the sink not serving yet, creates the Building `urn:ngsi-ld:Building:<ns>-None` of
/// [`building_named_in_datasets`] and the subscription `id` to every Building watching `name` with the
/// `attributeDeleted` trigger, notifying `application/json` in `form` (both requiring 201), then starts the sink,
/// deletes the `name` instances `selection` names (requiring 204) and returns the first notification the sink receives,
/// with the Building for the teardown and the cleanup that deletes the subscription.
pub(crate) async fn named_building_deletion_notification(
    namespace: &Namespace,
    id: &SubscriptionId,
    form: NotifiedAs,
    selection: InstanceSelection<'_>,
) -> Result<(ReceivedNotification, Entity<Input>, Cleanup)> {
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, namespace, &["Building", "name", "locatedAt"]).await?;
    let sink = NotificationSink::refusing()?;
    let building = building_named_in_datasets(namespace, "None")?;
    create_entity(&broker, &vocabulary, &building).await?;

    let endpoint_url = sink.endpoint(1)?;
    let endpoint = Endpoint::builder(endpoint_url.as_str()).accept(NgsiLdMediaType::Json).build()?;
    // The subscription owns its identifier and keeps it private; the teardown deletes it by the same one.
    let subscription = Subscription::builder()
        .id(id.clone())
        .entities([EntitySelector::new("Building".parse()?)])
        .watched_attributes(["name"])
        .notification(NotificationParams::builder(endpoint).format(Representation::Normalized).build()?)
        .notification_triggers([NotificationTrigger::AttributeDeleted])
        .build()?;
    let as_list = ld_json_body_client(&broker, &vocabulary)?;
    let subscribed = match form {
        NotifiedAs::Normalized => subscription_created(&as_list, &subscription).await?,
        NotifiedAs::KeyValues => create_subscription_document(&as_list, &subscription, &key_values_document(&subscription, &endpoint_url)?).await?,
    };
    let sink = sink.start_serving().await?;

    let as_link = broker.with_context(vocabulary.context())?;
    as_link
        .entities()
        .delete_attribute(&building.id, &"name".parse()?)
        .instances(selection)
        .send()
        .await?;
    Ok((sink.wait_for(1, 1).await?, building, subscribed))
}
