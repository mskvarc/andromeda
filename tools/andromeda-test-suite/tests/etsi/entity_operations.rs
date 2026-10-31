//! The entity operations the conformance tests send through the client: create in each `@context` placement, attribute
//! append, update, merge and partial update, retrieval as JSON, and the checks on their answers.

use andromeda_test_suite::{
    broker::BrokerClient,
    problem::answered,
    teardown::AnyAnswer,
    vocabulary::Vocabulary,
    wire::{content_type, header_value, json_body, request_to, send_to},
};
use bytes::Bytes;
use googletest::prelude::*;
use http::{HeaderValue, Method, Response, StatusCode, header::LOCATION};
use ngsi_ld::{
    client::{ClientError, ReqwestError},
    http::{
        ContextDelivery,
        UncheckedDocument,
        UncheckedRequest,
        operation::{CreateEntity, Overwrite, PartialAttributeUpdate, SystemAttributes},
    },
    model::{
        APPLICATION_JSON,
        AttributeFragment,
        AttributeKind,
        AttributeName,
        Change,
        Complete,
        Entity,
        EntityFragment,
        EntityId,
        Input,
        InstanceDeletion,
        Lossless,
    },
};
use serde_json::Value;
use std::fmt::Display;

/// The `observedAt` matched for a whole second in UTC: `2020-09-09T16:40:00Z`, with at most six fractional zeros.
pub(crate) const OBSERVED_AT_WHOLE_SECOND: &str = r"^2020-09-09T16:40:00(Z$|[.,](0{1,5}|0{6}\d*)Z)";

/// The `observedAt` text of the member `name` of `body`, when it is a string.
pub(crate) fn observed_at<'a>(body: &'a Value, name: &str) -> Option<&'a str> {
    body.get(name)?.get("observedAt")?.as_str()
}

/// `Create Entity Selecting Content Type` with `application/ld+json`: posts `entity` to `/entities/` with the
/// vocabulary as a one-element `@context` list in the body, and requires 201.
pub(crate) async fn create_entity(broker: &BrokerClient, vocabulary: &Vocabulary, entity: &Entity<Input>) -> Result<()> {
    create_entity_answer(broker, vocabulary, entity).await.map(|_| ())
}

/// The client of `broker` that sends the `@context` of `vocabulary` in every request body, as `application/ld+json`.
pub(crate) fn ld_json_body_client(broker: &BrokerClient, vocabulary: &Vocabulary) -> Result<BrokerClient> {
    Ok(broker
        .with_context(vocabulary.context_list())?
        .with_context_delivery(ContextDelivery::LdJsonBody))
}

/// Posts `entity` to `/entities/` with `client`, answering the broker's response whatever its status.
pub(crate) async fn send_create_entity(client: &BrokerClient, entity: &Entity<Input>) -> Result<Response<Bytes>, ClientError<ReqwestError>> {
    send_to(client, &CreateEntity::builder().entity(entity).build(), "entities/").await
}

/// `Create Entity Selecting Content Type` as [`create_entity`], answering the 201 response, whose head and body the
/// case reads.
pub(crate) async fn create_entity_answer(broker: &BrokerClient, vocabulary: &Vocabulary, entity: &Entity<Input>) -> Result<Response<Bytes>> {
    let client = ld_json_body_client(broker, vocabulary)?;
    let answer = send_create_entity(&client, entity).await;
    verify_that!(&answer, answered(StatusCode::CREATED))?;
    Ok(answer?)
}

/// `Create Entity` with the vocabulary named in the `Link` header and no `@context` in the body: posts `entity` to
/// `/entities/` as `application/json`, and requires 201.
pub(crate) async fn create_entity_with_link(broker: &BrokerClient, vocabulary: &Vocabulary, entity: &Entity<Input>) -> Result<()> {
    let client = broker.with_context(vocabulary.context())?;
    let answer = send_to(&client, &CreateEntity::builder().entity(entity).build(), "entities/").await;
    verify_that!(&answer, answered(StatusCode::CREATED))
}

/// `Create Entity` whose body names `vocabulary` as a plain `@context` string rather than a list: posts `entity` to
/// `/entities/` as `application/ld+json`, and requires 201.
pub(crate) async fn create_entity_with_context_string(broker: &BrokerClient, vocabulary: &Vocabulary, entity: &Entity<Input>) -> Result<()> {
    let client = broker.with_context(vocabulary.context())?.with_context_delivery(ContextDelivery::LdJsonBody);
    let answer = send_create_entity(&client, entity).await;
    verify_that!(&answer, answered(StatusCode::CREATED))
}

/// `Create Entity` of `entity` as [`create_entity_with_context_string`] does, answering the response whatever its
/// status: the setup of a case that names `HTTP *`.
pub(crate) async fn create_entity_with_context_string_any(broker: &BrokerClient, vocabulary: &Vocabulary, entity: &Entity<Input>) -> Result<()> {
    let client = broker.with_context(vocabulary.context())?.with_context_delivery(ContextDelivery::LdJsonBody);
    send_create_entity(&client, entity).await.any_answer();
    Ok(())
}

/// `Create Entity` with neither a `Link` nor an `@context`: posts `entity` to `/entities/` as `application/json`,
/// requires 201, and answers the response.
pub(crate) async fn create_entity_plain(broker: &BrokerClient, entity: &Entity<Input>) -> Result<Response<Bytes>> {
    let answer = send_to(broker, &CreateEntity::builder().entity(entity).build(), "entities/").await;
    verify_that!(&answer, answered(StatusCode::CREATED))?;
    Ok(answer?)
}

/// `Query Entities` for the entities `ids` of type `entity_type`, sent by `client` with the `@context` and `Accept` it
/// carries, answered as the JSON body of the response.
pub(crate) async fn query_entities_json(client: &BrokerClient, ids: Vec<EntityId>, entity_type: &str) -> Result<Value> {
    let received = client.with_responses().entities().query().ids(ids).entity_type(entity_type).send().await?;
    Ok(json_body(&received)?)
}

/// The answer of a create of `id` checked for the empty body and the `Location` naming the created
/// resource.
pub(crate) fn expect_created(answer: &Response<Bytes>, id: &impl Display) {
    expect_that!(answer.body().len(), eq(0));
    expect_that!(header_value(answer.headers(), &LOCATION), some(contains_substring(id.to_string())));
}

/// `DELETE entities/<id>/attrs/<name>?deleteAll=false`, sent as `application/json` in the context `client` carries.
/// The answer is not checked: the cases that delete an attribute only read what the deletion left behind.
pub(crate) async fn delete_attribute(client: &BrokerClient, id: &EntityId, name: &str) -> Result<()> {
    let deletion = request_to(client, Method::DELETE, &format!("entities/{id}/attrs/{name}?deleteAll=false"))?;
    client.send_unchecked(content_type(deletion, &APPLICATION_JSON)?).await.any_answer();
    Ok(())
}

/// `Merge Entity` of `id` with `fragment`, sent by `client`: requires 204 and an empty body.
pub(crate) async fn merge_entity(client: &BrokerClient, id: &EntityId, fragment: &EntityFragment<Complete>) -> Result<()> {
    let merged = client.with_responses().entities().merge(id, fragment).send().await;
    verify_that!(&merged, answered(StatusCode::NO_CONTENT))?;
    expect_that!(merged?.body.len(), eq(0));
    Ok(())
}

/// `Append Attributes` of `fragment` to `id` with `overwrite`, sent by `client`: requires 204 and an empty body.
pub(crate) async fn append_attributes(client: &BrokerClient, id: &EntityId, fragment: &EntityFragment, overwrite: Overwrite) -> Result<()> {
    let appended = client
        .with_responses()
        .entities()
        .append_attributes(id, fragment)
        .overwrite(overwrite)
        .send()
        .await;
    verify_that!(&appended, answered(StatusCode::NO_CONTENT))?;
    expect_that!(appended?.body.len(), eq(0));
    Ok(())
}

/// `Update Attributes` of `fragment` to `id`, sent by `client`: requires 204 and an empty body.
pub(crate) async fn update_attributes(client: &BrokerClient, id: &EntityId, fragment: &EntityFragment) -> Result<()> {
    let updated = client.with_responses().entities().update_attributes(id, fragment).send().await;
    verify_that!(&updated, answered(StatusCode::NO_CONTENT))?;
    expect_that!(updated?.body.len(), eq(0));
    Ok(())
}

/// `Partial Attribute Update` of `name` on `id` with `attribute`, sent by `client` in `form`: requires 204 and an
/// empty body.
pub(crate) async fn partial_update(
    client: &BrokerClient,
    id: &EntityId,
    name: &AttributeName,
    attribute: Change<AttributeFragment, InstanceDeletion>,
    form: Lossless,
) -> Result<()> {
    let updated = client.with_responses().entities().update_attribute(id, name, attribute).form(form).send().await;
    verify_that!(&updated, answered(StatusCode::NO_CONTENT))?;
    expect_that!(updated?.body.len(), eq(0));
    Ok(())
}

/// The `Partial Attribute Update` of `name` on `id` as `client` sends it with `document` as its body, in the context
/// `client` carries. The typed operation supplies the target and the headers. The attribute it names is a placeholder,
/// since `document` replaces the body it would write.
pub(crate) fn partial_update_request(client: &BrokerClient, id: &EntityId, name: &AttributeName, document: &UncheckedDocument) -> Result<UncheckedRequest> {
    let placeholder = Change::Delete(InstanceDeletion::new(AttributeKind::Relationship));
    let operation = PartialAttributeUpdate::builder().id(id).name(name).attribute(&placeholder).build();
    Ok(client.unchecked(&operation)?.document(document))
}

/// `Partial Attribute Update` of `name` on `id` with `document` as the body, sent by `client` in the context it
/// carries: requires 204 and an empty body.
pub(crate) async fn partial_update_document(client: &BrokerClient, id: &EntityId, name: &AttributeName, document: &UncheckedDocument) -> Result<()> {
    let answer = client.send_unchecked(partial_update_request(client, id, name, document)?).await;
    verify_that!(&answer, answered(StatusCode::NO_CONTENT))?;
    expect_that!(answer?.body().len(), eq(0));
    Ok(())
}

/// The `Link` header naming `vocabulary`'s `@context`, for a request whose body carries none.
pub(crate) fn context_link(vocabulary: &Vocabulary) -> Result<HeaderValue> {
    Ok(HeaderValue::from_str(&format!(
        "<{}>; rel=\"http://www.w3.org/ns/json-ld#context\"; type=\"application/ld+json\"",
        vocabulary.url()
    ))?)
}

/// The teardown of a case: `Delete Entity` and `Delete Temporal Entity` for each of `ids`, whose answers are ignored.
pub(crate) async fn delete_entities(broker: &BrokerClient, ids: &[&EntityId]) {
    for id in ids {
        broker.entities().delete(id).send().await.any_answer();
        broker.temporal().delete(id).send().await.any_answer();
    }
}

/// `Retrieve Entity` of `id` with the `@context` and `Accept` that `client` carries, answered as the JSON body of the
/// response.
pub(crate) async fn retrieve_entity_json(client: &BrokerClient, id: &EntityId) -> Result<Value> {
    let received = client.with_responses().entities().retrieve(id).send().await?;
    Ok(json_body(&received)?)
}

/// `Retrieve Entity` of `id` with `options=sysAttrs` and the `@context` and `Accept` that `client` carries, answered
/// as the JSON body of the response.
pub(crate) async fn retrieve_entity_sys_attrs_json(client: &BrokerClient, id: &EntityId) -> Result<Value> {
    let received = client
        .with_responses()
        .entities()
        .retrieve(id)
        .system_attributes(SystemAttributes::Include)
        .send()
        .await?;
    Ok(json_body(&received)?)
}

/// The member `name` of `body` when it is an array, object or string that holds at least one item, character or
/// member, as the `length($.<name>)>0` filter of a `JSONPath` reads it; `None` for any other member or a missing one.
pub(crate) fn non_empty_member<'a>(body: &'a Value, name: &str) -> Option<&'a Value> {
    body.get(name).filter(|member| match member {
        Value::Array(items) => !items.is_empty(),
        Value::Object(members) => !members.is_empty(),
        Value::String(text) => !text.is_empty(),
        Value::Null | Value::Bool(_) | Value::Number(_) => false,
    })
}
