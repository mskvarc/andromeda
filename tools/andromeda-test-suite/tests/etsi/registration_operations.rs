//! The Context Source Registration operations of the conformance tests: create, update, query, retrieve and delete,
//! with the context sources their endpoints name.

use crate::{
    entity_operations::{context_link, ld_json_body_client},
    registration_payloads::{building_registration, offstreet_pattern, vehicle_parking_registration},
    unchecked_requests::refused,
};
use andromeda_test_suite::{
    broker,
    broker::BrokerClient,
    cleanup::Cleanup,
    comparison::matches_document,
    context_source::ContextSource,
    namespace::Namespace,
    problem::answered,
    vocabulary::Vocabulary,
    wire::{header_value, json_body, request_to, send_to},
};
use bytes::Bytes;
use googletest::prelude::*;
use http::{
    Method,
    Response,
    StatusCode,
    header::{LINK, LOCATION},
};
use ngsi_ld::{
    client::{ClientError, ReqwestError},
    http::{
        UncheckedDocument,
        UncheckedRequest,
        headers::Accept,
        operation::{RegisterContextSource, UpdateRegistration},
    },
    model::{ErrorType, Input},
    registration::{CSourceRegistration, RegistrationFragment, RegistrationId},
};
use serde_json::{Value, json};
use std::fmt::Debug;

/// Posts `registration` to `/csourceRegistrations` with `client`, answering the broker's response whatever its status.
pub(crate) async fn send_registration(client: &BrokerClient, registration: &CSourceRegistration<Input>) -> Result<Response<Bytes>, ClientError<ReqwestError>> {
    send_to(client, &RegisterContextSource { registration }, "csourceRegistrations").await
}

/// `Create Context Source Registration`: posts `registration` to `/csourceRegistrations` with the vocabulary as a
/// one-element `@context` list in the body, requires 201, and requires the `Location` of the answer to name the
/// registration's id; answers the cleanup that deletes the registration.
pub(crate) async fn create_registration(broker: &BrokerClient, vocabulary: &Vocabulary, registration: &CSourceRegistration<Input>) -> Result<Cleanup> {
    let cleanup = Cleanup::of(broker).registration_of(registration);
    let client = ld_json_body_client(broker, vocabulary)?;
    let answer = send_registration(&client, registration).await;
    verify_that!(&answer, answered(StatusCode::CREATED))?;
    let id = registration.id().or_fail()?;
    expect_that!(header_value(answer?.headers(), &LOCATION), some(contains_substring(id.to_string())));
    Ok(cleanup)
}

/// The endpoint `sources/<ns>/<host>` of a fresh context source: the address under the mock origin the context source
/// tests register.
pub(crate) async fn csource_endpoint(namespace: &Namespace, host: &str) -> Result<url::Url> {
    Ok(ContextSource::start().await?.endpoint(&format!("sources/{namespace}/{host}"))?)
}

/// `Update Context Source Registration` of `id` with `document` as its body, sent by `client` in the context it
/// carries. The typed operation supplies the target and the headers; the empty fragment it names is replaced by
/// `document`.
pub(crate) fn update_registration_request(client: &BrokerClient, id: &RegistrationId, document: &UncheckedDocument) -> Result<UncheckedRequest> {
    let placeholder = RegistrationFragment::builder().build()?;
    Ok(client.unchecked(&UpdateRegistration { id, fragment: &placeholder })?.document(document))
}

/// Registers `terms` with `client`, then in that context the Vehicle
/// and parking registration `urn:ngsi-ld:ContextSourceRegistration:<ns>-1` and the [`building_registration`] `-2`,
/// both at the context source `sources/<ns>/my.csource.org:1026` (201 each); answers the vocabulary, the endpoint and
/// the cleanup that deletes both registrations.
pub(crate) async fn create_discovery_registrations(client: &BrokerClient, namespace: &Namespace, terms: &[&str]) -> Result<(Vocabulary, url::Url, Cleanup)> {
    let vocabulary = Vocabulary::register(client, namespace, terms).await?;
    let endpoint = csource_endpoint(namespace, "my.csource.org:1026").await?;
    let writer = ld_json_body_client(client, &vocabulary)?;
    let vehicle = vehicle_parking_registration(namespace, &endpoint, namespace.urn("Vehicle", "A456")?, None)?;
    let building = building_registration(namespace, 2, &endpoint)?;
    let cleanup = Cleanup::of(client).registration_of(&vehicle).registration_of(&building);
    writer.registrations().register(&vehicle).send().await?;
    writer.registrations().register(&building).send().await?;
    Ok((vocabulary, endpoint, cleanup))
}

/// `Query Context Source Registrations` written as the target `path`, with `vocabulary` in the `Link` header: requires
/// 400 `BadRequestData` and a title. The target is hand-written because the typed query refuses each invalid one.
pub(crate) async fn refused_registration_query(broker: &BrokerClient, vocabulary: &Vocabulary, path: &str) -> Result<()> {
    let request = request_to(broker, Method::GET, path)?.append_header(LINK, context_link(vocabulary)?);
    refused(broker, request, ErrorType::BadRequestData).await
}

/// The Retrieve Context Source Registration cases of the Vehicle and parking registration `-1`: registers `terms`
/// and the registration at the context source `sources/<ns>/my.csource.org:1026`, retrieves it with `Accept: */*`
/// (in the vocabulary's `Link` context when `context` is [`RetrievalContext::Link`]), requires 200 and the
/// registration with its terms compacted or expanded as `context` gives them, and deletes it (204).
pub(crate) async fn expect_vehicle_parking_retrieval(namespace: &Namespace, terms: &[&str], context: RetrievalContext) -> Result<()> {
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, namespace, terms).await?;
    let endpoint = csource_endpoint(namespace, "my.csource.org:1026").await?;
    let registration = vehicle_parking_registration(namespace, &endpoint, namespace.urn("Vehicle", "A456")?, None)?;
    let _registration = create_registration(&broker, &vocabulary, &registration).await?;
    let id = registration.id().or_fail()?;
    let reader = match context {
        RetrievalContext::Link => broker.with_context(vocabulary.context())?.with_accept(Accept::any()),
        RetrievalContext::Default => broker.with_accept(Accept::any()),
    };
    let retrieved = reader.with_responses().registrations().retrieve(id).send().await;
    verify_that!(&retrieved, answered(StatusCode::OK))?;
    let body = json_body(&retrieved?)?;
    let term = |name: &str| -> Result<String> {
        match context {
            RetrievalContext::Link => Ok(name.to_owned()),
            RetrievalContext::Default => Ok(namespace.expanded_term(name)?),
        }
    };
    let parking = term("OffStreetParking")?;
    expect_that!(
        &body,
        matches_document(json!({
            "id": id.to_string(),
            "type": "ContextSourceRegistration",
            "endpoint": endpoint.as_str(),
            "information": [
                {
                    "entities": [{"id": namespace.urn_text("Vehicle", "A456"), "type": term("Vehicle")?}],
                    "propertyNames": [term("brandName")?, term("speed")?],
                    "relationshipNames": [term("isParked")?]
                },
                {
                    "entities": [
                        {"idPattern": offstreet_pattern(namespace, ".*downtown$")?.as_ref(), "type": parking},
                        {"idPattern": offstreet_pattern(namespace, ".*47$")?.as_ref(), "type": parking}
                    ],
                    "propertyNames": ["availableSpotNumber", "totalSpotNumber"],
                    "relationshipNames": ["isNextToBuilding"]
                }
            ],
        }))
        .hiding_everywhere("@context")
        .hiding_everywhere("mode")
        .hiding_everywhere("operations")
    );
    delete_registrations(&broker, namespace, &[1]).await
}

/// The JSON-LD context a Retrieve Context Source Registration case reads the registration in.
#[derive(Debug, Clone, Copy)]
pub(crate) enum RetrievalContext {
    /// The vocabulary, named in a `Link` header: the vocabulary's terms come back compacted.
    Link,
    /// No `Link` header: the core context, under which the vocabulary's terms come back expanded.
    Default,
}

/// Deletes `urn:ngsi-ld:ContextSourceRegistration:<ns>-<n>` for each
/// `n` of `suffixes` with `client`, requiring 204 for each.
pub(crate) async fn delete_registrations(client: &BrokerClient, namespace: &Namespace, suffixes: &[u32]) -> Result<()> {
    for suffix in suffixes {
        let id: RegistrationId = namespace.urn_text("ContextSourceRegistration", suffix).parse()?;
        let deleted = client.with_responses().registrations().delete(&id).send().await;
        verify_that!(&deleted, answered(StatusCode::NO_CONTENT))?;
    }
    Ok(())
}

/// The [`building_registration`] `suffix` at `endpoint` as a query answers it in the vocabulary's context, without
/// `@context`.
pub(crate) fn building_registration_json(namespace: &Namespace, suffix: u32, endpoint: &url::Url) -> Value {
    json!({
        "id": namespace.urn_text("ContextSourceRegistration", suffix),
        "type": "ContextSourceRegistration",
        "information": [{
            "entities": [{"type": "Building"}],
            "propertyNames": ["name", "subCategory"],
            "relationshipNames": ["locatedAt"]
        }],
        "endpoint": endpoint.as_str(),
    })
}
