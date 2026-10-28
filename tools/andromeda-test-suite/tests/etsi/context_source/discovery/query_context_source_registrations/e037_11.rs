//! ETSI `037_11`: a query of context source registrations is paged by `limit` and `offset`, with the `prev` and
//! `next` links of clause 5.5.9 (clause 5.10.2).

use crate::{
    suite_keywords::{create_registration, csource_endpoint, delete_registrations, expect_page_links, page_link},
    suite_payloads::{VEHICLE_PARKING_BUILDING_TERMS, building_registration, point, vehicle_parking_registration},
};
use andromeda_test_suite::{
    broker::{self, BrokerClient},
    cleanup::Cleanup,
    namespace::Namespace,
    problem::answered,
    vocabulary::Vocabulary,
    wire::json_body,
};
use bytes::Bytes;
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    client::Received,
    http::{Page, headers::RelationType},
    model::Output,
    registration::{CSourceRegistration, RegistrationId, RegistrationInfo},
};
use serde_json::{Value, json};

/// The media type the broker gives the page links of a query answered as `application/json`.
const JSON: &str = "application/json";

/// The cases' setup: the Vehicle and parking registration `-1`, the registration `-2` of every `Building` located at
/// the point -8.521, 41.2 and the [`building_registration`] `-3`; answers the vocabulary and the cleanup that deletes
/// the three registrations.
async fn created(broker: &BrokerClient, namespace: &Namespace) -> Result<(Vocabulary, Cleanup)> {
    let vocabulary = Vocabulary::register(broker, namespace, &VEHICLE_PARKING_BUILDING_TERMS).await?;
    let endpoint = csource_endpoint(namespace, "my.csource.org:1026").await?;
    let vehicle = vehicle_parking_registration(namespace, &endpoint, namespace.urn("Vehicle", "A456")?, None)?;
    let registrations = create_registration(broker, &vocabulary, &vehicle).await?;
    let located = CSourceRegistration::builder(endpoint.as_str())
        .id(namespace.urn_text("ContextSourceRegistration", 2).parse::<RegistrationId>()?)
        .information([RegistrationInfo::builder().entities(["Building"]).build()?])
        .location(point(-8.521, 41.2)?)
        .build()?;
    let registrations = registrations
        .absorbing(create_registration(broker, &vocabulary, &located).await?)
        .absorbing(create_registration(broker, &vocabulary, &building_registration(namespace, 3, &endpoint)?).await?);
    Ok((vocabulary, registrations))
}

/// Queries the registrations of `Building` in `vocabulary`'s context with `limit` and `offset`, requiring 200.
async fn page(broker: &BrokerClient, vocabulary: &Vocabulary, limit: u32, offset: u32) -> Result<Received<Page<CSourceRegistration<Output>>>> {
    let received = broker
        .with_context(vocabulary.context())?
        .with_responses()
        .registrations()
        .query()
        .entity_type("Building")
        .limit(limit)
        .offset(offset)
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    Ok(received?)
}

/// The `type` of each registration of `body`: of each element of a list, or of the body itself when it is a single
/// object, `null` where it has none.
fn registration_types(body: &Bytes) -> Result<Vec<Value>> {
    let body: Value = serde_json::from_slice(body)?;
    let types = match body.as_array() {
        Some(items) => items.iter().map(|item| item.get("type").cloned().unwrap_or(Value::Null)).collect(),
        None => vec![body.get("type").cloned().unwrap_or(Value::Null)],
    };
    Ok(types)
}

/// Pins: Check that one can query context source registrations with providing page and limit parameters, pagination
/// logic shall be in place as mandated by clause 5.5.9.
/// Clause: 5.10.2
/// Corrected: `fix(031_02, 037_11, 041_03)`: compare page links by their limit and offset only (6.3.10, 5.5.9.2)
/// Corrected: `fix(037_11)`: retarget csr pagination to real pages and check only spec relations (5.5.9)
/// Changed: Check Pagination Prev And Next Headers reads the Link field as RequestsLibrary hands it over, every field
/// line joined with ", ", splits it on commas, drops the blank items and those holding rel="first", rel="last" or
/// json-ld#context, reduces the query of each remaining Link Target to its limit and offset parameters, and compares
/// the rest with the expected prev and next links as a multiset; the port requires at most one Link field line, which
/// a broker sending its link-values on several Link lines (RFC 9110 clause 5.3, RFC 8288 clause 3) fails
/// Changed: the Link field may not contain a backslash or `'''` nor end with `'`, as the keyword's Python literal
/// reads it
/// Changed: each idPattern is prefixed with the test's namespace (`urn:ngsi-ld:OffStreetParking:<ns>-`), so it matches
/// only the test's own entities.
/// Changed: the teardown deletes are held to the 204 a deleted registration answers, not only to a status below 400.
#[gtest]
#[tokio::test]
async fn e037_11_01_query_first_registration() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let (vocabulary, _registrations) = created(&broker, &namespace).await?;
    let received = page(&broker, &vocabulary, 1, 0).await?;
    expect_that!(registration_types(&received.body)?, elements_are![eq(&json!("ContextSourceRegistration"))]);
    expect_page_links(
        received.head.headers(),
        &[page_link("/ngsi-ld/v1/csourceRegistrations?limit=1&offset=1", RelationType::Next, JSON)?],
    )?;
    delete_registrations(&broker, &namespace, &[1, 2, 3]).await
}

/// Pins: Check that one can query context source registrations with providing page and limit parameters, pagination
/// logic shall be in place as mandated by clause 5.5.9.
/// Clause: 5.10.2
/// Corrected: `fix(031_02, 037_11, 041_03)`: compare page links by their limit and offset only (6.3.10, 5.5.9.2)
/// Corrected: `fix(037_11)`: retarget csr pagination to real pages and check only spec relations (5.5.9)
/// Changed: Check Pagination Prev And Next Headers reads the Link field as RequestsLibrary hands it over, every field
/// line joined with ", ", splits it on commas, drops the blank items and those holding rel="first", rel="last" or
/// json-ld#context, reduces the query of each remaining Link Target to its limit and offset parameters, and compares
/// the rest with the expected prev and next links as a multiset; the port requires at most one Link field line, which
/// a broker sending its link-values on several Link lines (RFC 9110 clause 5.3, RFC 8288 clause 3) fails
/// Changed: the Link field may not contain a backslash or `'''` nor end with `'`, as the keyword's Python literal
/// reads it
/// Changed: each idPattern is prefixed with the test's namespace (`urn:ngsi-ld:OffStreetParking:<ns>-`), so it matches
/// only the test's own entities.
/// Changed: the teardown deletes are held to the 204 a deleted registration answers, not only to a status below 400.
#[gtest]
#[tokio::test]
async fn e037_11_02_query_last_registration() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let (vocabulary, _registrations) = created(&broker, &namespace).await?;
    let received = page(&broker, &vocabulary, 1, 1).await?;
    expect_that!(registration_types(&received.body)?, elements_are![eq(&json!("ContextSourceRegistration"))]);
    expect_page_links(
        received.head.headers(),
        &[page_link("/ngsi-ld/v1/csourceRegistrations?limit=1&offset=0", RelationType::Prev, JSON)?],
    )?;
    delete_registrations(&broker, &namespace, &[1, 2, 3]).await
}

/// Pins: Check that one can query context source registrations with providing page and limit parameters, pagination
/// logic shall be in place as mandated by clause 5.5.9.
/// Clause: 5.10.2
/// Corrected: `fix(031_02, 037_11, 041_03)`: compare page links by their limit and offset only (6.3.10, 5.5.9.2)
/// Corrected: `fix(037_11)`: retarget csr pagination to real pages and check only spec relations (5.5.9)
/// Changed: Check Pagination Prev And Next Headers reads the Link field as RequestsLibrary hands it over, every field
/// line joined with ", ", splits it on commas, drops the blank items and those holding rel="first", rel="last" or
/// json-ld#context, reduces the query of each remaining Link Target to its limit and offset parameters, and compares
/// the rest with the expected prev and next links as a multiset; the port requires at most one Link field line, which
/// a broker sending its link-values on several Link lines (RFC 9110 clause 5.3, RFC 8288 clause 3) fails
/// Changed: the Link field may not contain a backslash or `'''` nor end with `'`, as the keyword's Python literal
/// reads it
/// Changed: each idPattern is prefixed with the test's namespace (`urn:ngsi-ld:OffStreetParking:<ns>-`), so it matches
/// only the test's own entities.
/// Changed: the teardown deletes are held to the 204 a deleted registration answers, not only to a status below 400.
#[gtest]
#[tokio::test]
async fn e037_11_03_query_all_registrations() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let (vocabulary, _registrations) = created(&broker, &namespace).await?;
    let received = page(&broker, &vocabulary, 15, 0).await?;
    let body = json_body(&received)?;
    expect_that!(body.as_array().map(Vec::len), some(eq(2)));
    expect_that!(body.pointer("/0/type"), some(eq(&json!("ContextSourceRegistration"))));
    expect_that!(body.pointer("/1/type"), some(eq(&json!("ContextSourceRegistration"))));
    expect_page_links(received.head.headers(), &[])?;
    delete_registrations(&broker, &namespace, &[1, 2, 3]).await
}
