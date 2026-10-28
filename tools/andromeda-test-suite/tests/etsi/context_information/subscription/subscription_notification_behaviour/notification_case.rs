//! The steps the notification cases of the ETSI `046` Robot runs share: the Building that is created, the subscription that
//! watches it, the `airQualityLevel` update that triggers the notification, and the teardown.

use crate::{
    suite_keywords::{create_subscription_document, delete_entities, send_create_entity, simplified_notification},
    suite_payloads::{dataset, key_values_document_reporting},
};
use andromeda_test_suite::{
    broker::BrokerClient,
    cleanup::Cleanup,
    namespace::Namespace,
    notification_sink::{NotificationSink, ReceivedNotification},
    problem::answered,
    teardown::AnyAnswer,
    wire::send_to,
};
use bytes::Bytes;
use cefact_units::UnitCode;
use googletest::prelude::*;
use http::{Response, StatusCode};
use ngsi_ld::{
    client::{ClientError, ReqwestError},
    http::operation::{CreateEntity, CreateSubscription, UpdateAttributes},
    model::{Entity, EntityFragment, EntityId, Input, Property},
    query::EntitySelector,
    subscription::{Subscription, SubscriptionId, SystemAttributeReporting},
};
use serde_json::Value;
use std::time::Duration;
use tokio::time::sleep;

/// The text pattern of the Hurl `matches` lines that check a notification or subscription instant: a date and a time in
/// UTC, with a fraction of any length or none.
pub(crate) const INSTANT_PATTERN: &str = r"^(([1-9][0-9]{3}|0[1-9][0-9]{2}|00[1-9][0-9]|000[1-9])-((0[13578]|1[02])-(0[1-9]|[12][0-9]|3[01])|(0[469]|11)-(0[1-9]|[12][0-9]|30)|02-(0[1-9]|1[0-9]|2[0-8]))|([0-9]{2}(0[48]|[2468][048]|[13579][26])|(0[48]|[2468][048]|[13579][26])00)-02-29)T([01][0-9]|2[0-3]):[0-5][0-9]:[0-5][0-9]([.,][0-9]+Z|Z$)";

/// A system timestamp of an expected document: the comparison accepts any well-formed date there, whatever this
/// value.
pub(crate) const ANY_SYSTEM_TIME: &str = "1970-01-01T00:00:00Z";

/// Creates `building` with `client`, requiring 201, then waits one second before the next step.
pub(crate) async fn created(client: &BrokerClient, building: &Entity<Input>) -> Result<()> {
    let answer = send_to(client, &CreateEntity::builder().entity(building).build(), "entities/").await;
    verify_that!(&answer, answered(StatusCode::CREATED))?;
    sleep(Duration::from_secs(1)).await;
    Ok(())
}

/// Creates `subscription` with `client`, requiring 201; answers the cleanup that deletes it.
pub(crate) async fn subscription_created(client: &BrokerClient, subscription: &Subscription<Input>) -> Result<Cleanup> {
    let cleanup = Cleanup::of(client).subscription_of(subscription);
    let answer = send_to(client, &CreateSubscription { subscription }, "subscriptions/").await;
    verify_that!(&answer, answered(StatusCode::CREATED))?;
    Ok(cleanup)
}

/// Creates `subscription` with `client`, requiring 201, then waits one second before the next step; answers the
/// cleanup that deletes it.
pub(crate) async fn subscribed(client: &BrokerClient, subscription: &Subscription<Input>) -> Result<Cleanup> {
    let cleanup = subscription_created(client, subscription).await?;
    sleep(Duration::from_secs(1)).await;
    Ok(cleanup)
}

/// Updates the `airQualityLevel` of `building` to 5 with `client`, answering the broker's response whatever its status.
pub(crate) async fn air_quality_updated(client: &BrokerClient, building: &Entity<Input>) -> Result<()> {
    let fragment = EntityFragment::builder()
        .property(
            "airQualityLevel",
            Property::builder(5).unit_code(UnitCode::C62).observed_at("2020-10-10T16:40:00.000Z".parse()?),
        )
        .build()?;
    attributes_updated(client, &building.id, &fragment).await.any_answer();
    Ok(())
}

/// The update of the `name` instance of the dataset `urn:ngsi-ld:Dataset:<ns>-french-name` to Le Tour Eiffel.
pub(crate) fn french_name_update(namespace: &Namespace) -> Result<EntityFragment> {
    Ok(EntityFragment::builder()
        .property("name", Property::builder("Le Tour Eiffel").dataset_id(dataset(namespace, "french-name")?))
        .build()?)
}

/// `Update Attributes` of `fragment` on `id`, sent with `client` to `entities/<id>/attrs/` as the ETSI keyword writes
/// it, answering the broker's response whatever its status.
pub(crate) async fn attributes_updated(client: &BrokerClient, id: &EntityId, fragment: &EntityFragment) -> Result<Response<Bytes>, ClientError<ReqwestError>> {
    let update = UpdateAttributes::builder().id(id).fragment(fragment).build();
    send_to(client, &update, &format!("entities/{id}/attrs/")).await
}

/// Creates `building` with `as_string` whatever the answer, then the keyValues subscription `id` to the sink's endpoint with its
/// system attributes reported as `reporting` says (requiring 201), then updates the `airQualityLevel` of `building` with
/// `as_list` and returns the first notification the endpoint receives, with the cleanup that deletes the subscription.
pub(crate) async fn first_notification_with_sys_attrs(
    as_string: &BrokerClient,
    as_list: &BrokerClient,
    sink: &NotificationSink,
    building: &Entity<Input>,
    id: &SubscriptionId,
    reporting: SystemAttributeReporting,
) -> Result<(ReceivedNotification, Cleanup)> {
    send_create_entity(as_string, building).await.any_answer();
    sleep(Duration::from_secs(1)).await;

    let endpoint_url = sink.endpoint(1)?;
    let subscription = Subscription::builder()
        .id(id.clone())
        .entities([EntitySelector::new("Building".parse()?).with_ids(building.id.clone())])
        .notification(simplified_notification(&endpoint_url)?)
        .build()?;
    let document = key_values_document_reporting(&subscription, &endpoint_url, reporting)?;
    let cleanup = create_subscription_document(as_list, &subscription, &document).await?;
    sleep(Duration::from_secs(1)).await;

    air_quality_updated(as_list, building).await?;
    Ok((sink.wait_for(1, 1).await?, cleanup))
}

/// Deletes the subscription `id` and the Building `building` the case created, whatever the answers.
pub(crate) async fn teardown(broker: &BrokerClient, id: &SubscriptionId, building: &Entity<Input>) {
    broker.subscriptions().delete(id).send().await.any_answer();
    delete_entities(broker, &[&building.id]).await;
}

/// The item of the notification's `data` whose `id` is `id`, as the ETSI keyword selects it.
pub(crate) fn data_item<'a>(body: &'a Value, id: &EntityId) -> Option<&'a Value> {
    let id = id.to_string();
    body.get("data")?
        .as_array()?
        .iter()
        .find(|item| item.get("id").and_then(Value::as_str) == Some(id.as_str()))
}

/// Every value of a member `name` at any depth of `value`, as the descendant selector `..name` finds them.
pub(crate) fn descendants<'a>(value: &'a Value, name: &str) -> Vec<&'a Value> {
    match value {
        Value::Object(members) => members
            .iter()
            .flat_map(|(key, member)| {
                let own = (key == name).then_some(member);
                own.into_iter().chain(descendants(member, name))
            })
            .collect(),
        Value::Array(items) => items.iter().flat_map(|item| descendants(item, name)).collect(),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => Vec::new(),
    }
}
