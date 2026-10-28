//! ETSI `037_10`: a query of context source registrations narrowed by entity ids alone is refused as too wide, and
//! one with an NGSI-LD query answers the registration whose Context Source Properties match (clause 5.10.2).

use crate::suite_keywords::{context_link, create_discovery_registrations, create_registration, delete_registrations};
use andromeda_test_suite::{
    broker::{self, BrokerClient},
    cleanup::Cleanup,
    comparison::matches_document,
    namespace::Namespace,
    problem::{answered, problem},
    vocabulary::Vocabulary,
    wire::{json_body, request_to},
};
use googletest::prelude::*;
use http::{Method, StatusCode, header::LINK};
use ngsi_ld::{
    model::ErrorType,
    query::q,
    registration::{CSourceRegistration, RegistrationId, RegistrationInfo},
};
use serde_json::json;
use url::Url;

/// The terms of the cases' `@context`: those of the discovery registrations and the two Context Source Properties.
const TERMS: [&str; 14] = [
    "Vehicle",
    "brandName",
    "speed",
    "isParked",
    "OffStreetParking",
    "availableSpotNumber",
    "totalSpotNumber",
    "isNextToBuilding",
    "Building",
    "name",
    "subCategory",
    "locatedAt",
    "csourceProperty1",
    "csourceProperty2",
];

/// The cases' setup: the two discovery registrations and the registration `-3` of every `Building` with the Context
/// Source Properties `csourceProperty1` `aValue` and `csourceProperty2` `anotherValue`; answers the vocabulary and the
/// endpoint and the cleanup that deletes the three registrations.
async fn created(client: &BrokerClient, namespace: &Namespace) -> Result<(Vocabulary, Url, Cleanup)> {
    let (vocabulary, endpoint, registrations) = create_discovery_registrations(client, namespace, &TERMS).await?;
    let third = CSourceRegistration::builder(endpoint.as_str())
        .id(namespace.urn_text("ContextSourceRegistration", 3).parse::<RegistrationId>()?)
        .information([RegistrationInfo::builder().entities(["Building"]).build()?])
        .property("csourceProperty1", "aValue")
        .property("csourceProperty2", "anotherValue")
        .build()?;
    let registrations = registrations.absorbing(create_registration(client, &vocabulary, &third).await?);
    Ok((vocabulary, endpoint, registrations))
}

/// Pins: A discovery query narrowed only by an entity id list is too wide (clause 5.10.2.4).
/// Clause: 5.10.2
/// Corrected: `fix(037_10_01)`: id-only discovery is too wide and rejected (5.10.2.4)
/// Changed: each idPattern is prefixed with the test's namespace (`urn:ngsi-ld:OffStreetParking:<ns>-`), so it matches
/// only the test's own entities.
/// Changed: the teardown deletes are held to the 204 a deleted registration answers, not only to a status below 400.
#[gtest]
#[tokio::test]
async fn e037_10_01_with_list_of_entity_ids() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let (vocabulary, _, _registrations) = created(&broker, &namespace).await?;
    // The typed query refuses to send a query narrowed by ids alone.
    let target = format!(
        "csourceRegistrations?id={}%2C{}",
        namespace.urn_text("ContextSourceRegistration", 1).replace(':', "%3A"),
        namespace.urn_text("ContextSourceRegistration", 3).replace(':', "%3A"),
    );
    let request = request_to(&broker, Method::GET, &target)?.append_header(LINK, context_link(&vocabulary)?);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    delete_registrations(&broker, &namespace, &[1, 2, 3]).await
}

/// Pins: Check that one can query context source registrations. If present, the conditions specified by the context
/// source query match the respective Context Source Properties
/// Clause: 5.10.2
/// Corrected: `fix(037_10_02)`: csr discovery omits the unstored location and inlines no @context (table 5.2.9-1,
/// 6.3.6)
/// Tenant: private (the case queries registrations by an NGSI-LD query, whose attributes clause 5.10.2.4 matches as a
/// list of attribute names alone, with no entity type; clause 5.10.2.4 matches every registration of the tenant whose
/// `RegistrationInfo` names that attribute or names no attribute at all ("If no Properties or Relationships are
/// specified in the `RegistrationInfo`, the Attribute names are considered matching"), so in the shared tenant other
/// files' registrations of entities alone are listed beside the case's own and the exact count fails; clause 4.14
/// keeps each tenant's registrations its own)
/// Changed: each idPattern is prefixed with the test's namespace (`urn:ngsi-ld:OffStreetParking:<ns>-`), so it matches
/// only the test's own entities.
/// Changed: the teardown deletes are held to the 204 a deleted registration answers, not only to a status below 400.
#[gtest]
#[tokio::test]
async fn e037_10_02_with_ngsi_ld_query() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let tenant = broker::private_tenant(&broker, &namespace).await?;
    let (vocabulary, endpoint, _registrations) = created(&tenant, &namespace).await?;
    let received = tenant
        .with_context(vocabulary.context())?
        .with_responses()
        .registrations()
        .query()
        .q(q::attr("csourceProperty1").eq("aValue"))
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    expect_that!(
        &body,
        matches_document(json!([{
            "id": namespace.urn_text("ContextSourceRegistration", 3),
            "type": "ContextSourceRegistration",
            "information": [{"entities": [{"type": "Building"}]}],
            "endpoint": endpoint.as_str(),
            "csourceProperty1": "aValue",
            "csourceProperty2": "anotherValue",
        }]))
        .hiding_everywhere("mode")
        .hiding_everywhere("operations")
    );
    delete_registrations(&tenant, &namespace, &[1, 2, 3]).await
}
