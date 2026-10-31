//! The context source of a distributed-operations test, the stubs that set up its replies, and the registration the
//! test forwards through.
//!
//! The source is the test's own server. It closes every connection until the test's registration exists, then answers
//! each request with the first stub whose [`ForkRule`] matches it, in the order the stubs were set. A request no stub
//! matches is answered 502 after five seconds.

use crate::{
    distributed_operations::fork_rule::{ForkRule, QueriedEntity},
    entity_operations::ld_json_body_client,
};
use andromeda_test_suite::{
    broker::BrokerClient,
    cleanup::Cleanup,
    context_source::ContextSource,
    namespace::Namespace,
    problem::answered,
    vocabulary::Vocabulary,
    wire::send_to,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::{
        ContextDelivery,
        UncheckedDocument,
        operation::{CreateEntity, RegisterContextSource},
    },
    model::{Entity, EntityTypeName, Input},
    registration::{CSourceRegistration, EntityInfo, OperationGroup, RegistrationId, RegistrationInfo, RegistrationMode},
};
use serde_json::{Value, json};
use std::time::Duration;
use url::Url;
use wiremock::{Match, Mock, MockBuilder, ResponseTemplate, matchers::any};

/// How long the source holds a request no stub matches before it answers: below the broker's 5 s wait for a
/// source, so the answer, and the request with it, is recorded before the broker answers the client.
const UNMATCHED_DELAY: Duration = Duration::from_secs(3);

/// Starts `source` serving, with nothing recorded of what it refused, and answering 502 after [`UNMATCHED_DELAY`] to
/// every request no stub matches.
pub(crate) async fn serve(source: ContextSource) -> Result<ContextSource> {
    let source = source.start_serving().await;
    let unmatched = ResponseTemplate::new(StatusCode::BAD_GATEWAY).set_delay(UNMATCHED_DELAY);
    source.mount(Mock::given(any()).respond_with(unmatched).with_priority(u8::MAX)).await?;
    Ok(source)
}

/// Answers every request `rule` matches with `status`, and with `body` as its JSON text and no
/// `Content-Type` when it names one.
pub(crate) async fn stub(source: &ContextSource, rule: ForkRule, status: StatusCode, body: Option<&Value>) -> Result<()> {
    mount_reply(source, Mock::given(rule), status, body).await
}

/// A stub of a `POST /entityOperations/query` as [`stub`] sets it, matching only a body whose first
/// queried entity `queried` names.
pub(crate) async fn stub_query_of(source: &ContextSource, rule: ForkRule, queried: QueriedEntity, status: StatusCode, body: Option<&Value>) -> Result<()> {
    mount_reply(source, Mock::given(rule).and(queried), status, body).await
}

/// Mounts the reply of `status` and `body` to the requests `given` matches.
async fn mount_reply(source: &ContextSource, given: MockBuilder, status: StatusCode, body: Option<&Value>) -> Result<()> {
    let reply = ResponseTemplate::new(status);
    let reply = match body {
        Some(body) => reply.set_body_bytes(serde_json::to_vec(body)?),
        None => reply,
    };
    source.mount(given.respond_with(reply)).await?;
    Ok(())
}

/// The stub rule of a retrieval of `urn:ngsi-ld:Vehicle:<ns>-<n>` at the source's `<below>`: the entity's path
/// without a query, or any target with a query that names the id.
pub(crate) fn vehicle_retrieval_rule(namespace: &Namespace, below: &str, n: u32) -> ForkRule {
    ForkRule::resource_or_tokens(
        Method::GET,
        &source_path(namespace, ""),
        &source_path(namespace, &format!("{below}/ngsi-ld/v1/entities/{}", namespace.urn_text("Vehicle", n))),
        &["urn", "ngsi-ld", "vehicle", &format!("{namespace}-{n}")],
    )
}

/// The registration `urn:ngsi-ld:ContextSourceRegistration:<ns>-<suffix>` of the Vehicle `urn:ngsi-ld:Vehicle:<ns>-1`
/// in `mode` at `endpoint` for the redirection operations, with the attributes `properties`.
pub(crate) fn vehicle_registration(
    namespace: &Namespace,
    suffix: u32,
    endpoint: &Url,
    mode: RegistrationMode,
    properties: &[&str],
) -> Result<CSourceRegistration<Input>> {
    let vehicle = [EntityInfo::new("Vehicle".parse::<EntityTypeName>()?).with_id(namespace.urn("Vehicle", 1)?)];
    let information = match properties {
        [] => RegistrationInfo::builder().entities(vehicle).build()?,
        names => RegistrationInfo::builder().entities(vehicle).property_names(names.iter().copied()).build()?,
    };
    Ok(CSourceRegistration::builder(endpoint.as_str())
        .id(registration_id(namespace, suffix)?)
        .information([information])
        .operations([OperationGroup::RedirectionOps])
        .mode(mode)
        .build()?)
}

/// `Create Entity` of `entity` at `/entities/?local=true`, as `application/ld+json` whose body names the vocabulary
/// as a plain `@context` string: requires 201.
pub(crate) async fn create_entity_locally(broker: &BrokerClient, vocabulary: &Vocabulary, entity: &Entity<Input>) -> Result<()> {
    let client = broker.with_context(vocabulary.context())?.with_context_delivery(ContextDelivery::LdJsonBody);
    let answer = send_to(&client, &CreateEntity::builder().entity(entity).build(), "entities/?local=true").await;
    verify_that!(&answer, answered(StatusCode::CREATED))
}

/// How many of the requests the source received on entity paths `rule` matches.
pub(crate) async fn stub_count(source: &ContextSource, rule: &ForkRule) -> Result<usize> {
    Ok(source.received().await?.iter().filter(|request| rule.matches(request)).count())
}

/// The path `/sources/<ns><below>` of the source, as the source's stubs and the registration's endpoint name it.
pub(crate) fn source_path(namespace: &Namespace, below: &str) -> String {
    format!("/sources/{namespace}{below}")
}

/// The endpoint `<source>/sources/<ns><below>` the case's registration names.
pub(crate) fn source_endpoint(source: &ContextSource, namespace: &Namespace, below: &str) -> Result<Url> {
    Ok(source.endpoint(&format!("sources/{namespace}{below}"))?)
}

/// The registration `urn:ngsi-ld:ContextSourceRegistration:<ns>-<suffix>` of every entity of `entity_type` at
/// `endpoint`, in `mode` when it names one.
pub(crate) fn type_registration(
    namespace: &Namespace,
    suffix: u32,
    entity_type: &str,
    endpoint: &Url,
    mode: Option<RegistrationMode>,
) -> Result<CSourceRegistration<Input>> {
    Ok(CSourceRegistration::builder(endpoint.as_str())
        .id(registration_id(namespace, suffix)?)
        .information([RegistrationInfo::builder().entities([entity_type]).build()?])
        .maybe_mode(mode)
        .build()?)
}

/// The id `urn:ngsi-ld:ContextSourceRegistration:<ns>-<suffix>`.
pub(crate) fn registration_id(namespace: &Namespace, suffix: u32) -> Result<RegistrationId> {
    Ok(namespace.urn_text("ContextSourceRegistration", suffix).parse()?)
}

/// `Create Context Source Registration` of `registration` as `application/ld+json` with the vocabulary as a
/// one-element `@context` list in the body: requires 201, and answers the cleanup that deletes the registration.
pub(crate) async fn register(broker: &BrokerClient, vocabulary: &Vocabulary, registration: &CSourceRegistration<Input>) -> Result<Cleanup> {
    let cleanup = Cleanup::of(broker).registration_of(registration);
    ld_json_body_client(broker, vocabulary)?.registrations().register(registration).send().await?;
    Ok(cleanup)
}

/// `Create Context Source Registration` of `registration` as [`register`] sends it, with its `mode` member written
/// as `mode`: the builder refuses an auxiliary registration that names no operations, which the cases register.
/// Answers the cleanup that deletes the registration.
pub(crate) async fn register_in_mode(broker: &BrokerClient, vocabulary: &Vocabulary, registration: &CSourceRegistration<Input>, mode: &str) -> Result<Cleanup> {
    let cleanup = Cleanup::of(broker).registration_of(registration);
    let client = ld_json_body_client(broker, vocabulary)?;
    let document = UncheckedDocument::try_from(registration.render())?.set_text("mode", mode);
    let request = client.unchecked(&RegisterContextSource { registration })?.document(&document);
    let answer = client.send_unchecked(request).await;
    verify_that!(&answer, answered(StatusCode::CREATED))?;
    Ok(cleanup)
}

/// `Delete Context Source Registration` of `urn:ngsi-ld:ContextSourceRegistration:<ns>-<suffix>`: requires 204.
pub(crate) async fn unregister(broker: &BrokerClient, namespace: &Namespace, suffix: u32) -> Result<()> {
    broker.registrations().delete(&registration_id(namespace, suffix)?).send().await?;
    Ok(())
}

/// The Vehicle `urn:ngsi-ld:Vehicle:<ns>-<n>` a source answers with: of `brand`, with the Relationship `parked` to
/// the `OffStreetParking` `<parking>`, observed at 2017-07-29T12:00:04Z and provided by the `Person` `<person>`.
pub(crate) fn vehicle_reply(namespace: &Namespace, n: u32, brand: &str, parked: &str, parking: &str, person: &str) -> Value {
    json!({
        "id": namespace.urn_text("Vehicle", n),
        "type": "Vehicle",
        "brandName": {"type": "Property", "value": brand},
        parked: {
            "type": "Relationship",
            "object": namespace.urn_text("OffStreetParking", parking),
            "observedAt": "2017-07-29T12:00:04Z",
            "providedBy": {"type": "Relationship", "object": namespace.urn_text("Person", person)},
        },
    })
}

/// The ids of the entities of the list `body`, in order.
pub(crate) fn listed_ids(body: &Value) -> Vec<&str> {
    body.as_array()
        .into_iter()
        .flatten()
        .filter_map(|entity| entity.get("id").and_then(Value::as_str))
        .collect()
}
