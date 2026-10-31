//! The temporal entity operations of the conformance tests, and the reading of the instances and the `Content-Range` of
//! their answers.

use andromeda_test_suite::{
    broker::BrokerClient,
    namespace::Namespace,
    problem::answered,
    teardown::AnyAnswer,
    vocabulary::Vocabulary,
    wire::{header_value, json_body},
};
use googletest::prelude::*;
use http::{HeaderMap, StatusCode, header::CONTENT_RANGE};
use ngsi_ld::{
    http::{
        ContextDelivery,
        TemporalUpsertOutcome,
        headers::Accept,
        operation::{RepresentationSpelling, SystemAttributes},
    },
    model::{EntityId, Input, InstanceId, evolution::TemporalEntity},
    query::temporal_query::TemporalProperty,
};
use serde_json::Value;

/// `Create Temporal Entity`: posts `evolution` to `/temporal/entities` with the vocabulary as a one-element `@context`
/// list in the body, and requires the creation, not an update of an existing evolution.
pub(crate) async fn create_temporal_entity(broker: &BrokerClient, vocabulary: &Vocabulary, evolution: &TemporalEntity<Input>) -> Result<()> {
    let client = broker
        .with_context(vocabulary.context_list())?
        .with_context_delivery(ContextDelivery::LdJsonBody);
    match client.temporal().create_or_update(evolution).send().await? {
        TemporalUpsertOutcome::Created { .. } => Ok(()),
        TemporalUpsertOutcome::Updated => fail!("the broker updated an existing temporal evolution instead of creating it"),
    }
}

/// The setup of a temporal case: registers `terms` in the namespace, creates `evolution` in their context, and answers
/// the vocabulary.
pub(crate) async fn create_temporal_evolution(
    broker: &BrokerClient,
    namespace: &Namespace,
    terms: &[&str],
    evolution: &TemporalEntity<Input>,
) -> Result<Vocabulary> {
    let vocabulary = Vocabulary::register(broker, namespace, terms).await?;
    create_temporal_entity(broker, &vocabulary, evolution).await?;
    Ok(vocabulary)
}

/// `GET temporal/entities/<id>?options=temporalValues&timeproperty=deletedAt` in the context `client` carries, without
/// an `Accept` header: requires 200 and answers the body as JSON.
pub(crate) async fn deleted_temporal_values(client: &BrokerClient, id: &EntityId) -> Result<Value> {
    let received = client
        .with_accept(Accept::any())
        .with_responses()
        .temporal()
        .retrieve_values(id)
        .time_property(TemporalProperty::DeletedAt)
        .spelling(RepresentationSpelling::default().in_options())
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    Ok(json_body(&received?)?)
}

/// `Retrieve Temporal Evolution` of `id` with the `@context` and `Accept` that `client` carries, answered as the JSON
/// body of the response.
pub(crate) async fn retrieve_temporal_json(client: &BrokerClient, id: &EntityId) -> Result<Value> {
    let received = client.with_responses().temporal().retrieve(id).send().await?;
    Ok(json_body(&received)?)
}

/// `Retrieve Temporal Evolution` of `id` with `options=sysAttrs` and the `@context` and `Accept` that `client`
/// carries, answered as the JSON body of the response.
pub(crate) async fn retrieve_temporal_sys_attrs_json(client: &BrokerClient, id: &EntityId) -> Result<Value> {
    let received = client
        .with_responses()
        .temporal()
        .retrieve(id)
        .system_attributes(SystemAttributes::Include)
        .send()
        .await?;
    Ok(json_body(&received)?)
}

/// The `instanceId` of the first `speed` instance of a temporal body, which the cases that delete an instance name.
pub(crate) fn first_speed_instance_id(body: &Value) -> Result<InstanceId> {
    Ok(body.pointer("/speed/0/instanceId").and_then(Value::as_str).or_fail()?.parse()?)
}

/// The teardown of a temporal case: `Delete Temporal Entity` for each of `ids`, whose answers are ignored.
pub(crate) async fn delete_temporal_entities(broker: &BrokerClient, ids: &[&EntityId]) {
    for id in ids {
        broker.temporal().delete(id).send().await.any_answer();
    }
}

/// A temporal `Content-Range` as written: `<unit> <start>-<end>/<size>`, each part as text.
pub(crate) struct ContentRangeText<'a> {
    /// The unit, `DateTime` for a temporal answer.
    pub(crate) unit: &'a str,
    /// The bound the answer names first: the least recent instant of a forwards answer, the most recent of a backwards
    /// one.
    pub(crate) start: &'a str,
    /// The bound the answer names last.
    pub(crate) end: &'a str,
    /// `*` for a forwards answer, or the `lastN` of a backwards one.
    pub(crate) size: &'a str,
}

impl<'a> ContentRangeText<'a> {
    /// Splits `value` at its unit, at the `Z-` between the bounds and at the last `/`; `None` when it has no such shape.
    fn parse(value: &'a str) -> Option<Self> {
        let (unit, rest) = value.split_once(' ')?;
        let (range, size) = rest.rsplit_once('/')?;
        let divider = range.find("Z-")?;
        Some(Self {
            unit,
            start: range.get(..=divider)?,
            end: range.get(divider + 2..)?,
            size,
        })
    }
}

/// The `Content-Range` of a partial answer, when the header is present once and reads as text.
pub(crate) fn content_range(headers: &HeaderMap) -> Option<ContentRangeText<'_>> {
    header_value(headers, &CONTENT_RANGE).and_then(ContentRangeText::parse)
}

/// How an attribute of a retrieved evolution carries its instants.
#[derive(Clone, Copy)]
pub(crate) enum InstantForm {
    /// Each instance is an object with an `observedAt` member.
    ObservedAtMember,
    /// The simplified representation: `values` holds `[value, observedAt]` pairs.
    ValuePair,
}

impl InstantForm {
    /// The `observedAt` text of `instance`, which the form holds as a member or as the second item of a pair.
    fn observed_at(self, instance: &Value) -> Option<&str> {
        match self {
            Self::ObservedAtMember => instance.get("observedAt")?.as_str(),
            Self::ValuePair => instance.get(1)?.as_str(),
        }
    }
}

/// The list that carries the instances of `attribute` in `body`: the attribute itself, or its `values` in the
/// simplified form.
pub(crate) fn instance_list<'a>(body: &'a Value, attribute: &str, form: InstantForm) -> Option<&'a Value> {
    let attribute = body.get(attribute)?;
    match form {
        InstantForm::ObservedAtMember => Some(attribute),
        InstantForm::ValuePair => attribute.get("values"),
    }
}

/// The `observedAt` texts of the instances of `attribute` in `body`, when every instance carries one.
pub(crate) fn instants<'a>(body: &'a Value, attribute: &str, form: InstantForm) -> Option<Vec<&'a str>> {
    instance_list(body, attribute, form)?
        .as_array()?
        .iter()
        .map(|instance| form.observed_at(instance))
        .collect()
}
