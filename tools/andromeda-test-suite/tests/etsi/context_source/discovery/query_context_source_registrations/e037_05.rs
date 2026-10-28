//! ETSI `037_05`: a query of context source registrations by type and idPattern answers the registrations whose
//! `EntityInfo` matches, trimmed to the matching `RegistrationInfo`, and those that name no entities (clause 5.10.2).

use crate::{
    suite_keywords::{create_registration, csource_endpoint, delete_registrations},
    suite_payloads::{VEHICLE_PARKING_TERMS, offstreet_pattern, vehicle_parking_registration},
};
use andromeda_test_suite::{
    broker::{self, BrokerClient},
    comparison::matches_document,
    namespace::Namespace,
    problem::answered,
    vocabulary::Vocabulary,
    wire::json_body,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::registration::{CSourceRegistration, RegistrationId, RegistrationInfo};
use serde_json::{Value, json};

/// Queries the registrations of `OffStreetParking` whose id matches `urn:ngsi-ld:OffStreetParking:<ns>-.*downtown$`
/// in `vocabulary`'s context with `client`: requires 200 and answers the body.
async fn downtown_parkings(client: &BrokerClient, vocabulary: &Vocabulary, namespace: &Namespace) -> Result<Value> {
    let received = client
        .with_context(vocabulary.context())?
        .with_responses()
        .registrations()
        .query()
        .entity_type("OffStreetParking")
        .id_pattern(offstreet_pattern(namespace, ".*downtown$")?)
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    Ok(json_body(&received?)?)
}

/// Pins: Check that one can query context source registrations matching `EntityInfo` of `RegistrationInfo`
/// Clause: 5.10.2
/// Corrected: fix(csr-discovery-trim): trim discovery coverage to matching registrationinfo (5.10.2.5)
/// Changed: each idPattern, the query's included, is prefixed with the test's namespace
/// (`urn:ngsi-ld:OffStreetParking:<ns>-`), so it matches only the test's own entities.
/// Changed: the teardown delete is held to the 204 a deleted registration answers, not only to a status below 400.
#[gtest]
#[tokio::test]
async fn e037_05_01_registration_with_entity_info_matching_the_query() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &VEHICLE_PARKING_TERMS).await?;
    let endpoint = csource_endpoint(&namespace, "my.csource.org:1026").await?;
    let registration = vehicle_parking_registration(&namespace, &endpoint, namespace.urn("Vehicle", "A456")?, None)?;
    let _registration = create_registration(&broker, &vocabulary, &registration).await?;

    let body = downtown_parkings(&broker, &vocabulary, &namespace).await?;
    let downtown = offstreet_pattern(&namespace, ".*downtown$")?;
    let forty_seven = offstreet_pattern(&namespace, ".*47$")?;
    expect_that!(
        &body,
        matches_document(json!([{
            "id": namespace.urn_text("ContextSourceRegistration", 1),
            "type": "ContextSourceRegistration",
            "information": [{
                "entities": [
                    {"idPattern": downtown.as_ref(), "type": "OffStreetParking"},
                    {"idPattern": forty_seven.as_ref(), "type": "OffStreetParking"}
                ],
                "propertyNames": ["availableSpotNumber", "totalSpotNumber"],
                "relationshipNames": ["isNextToBuilding"]
            }],
            "endpoint": endpoint.as_str(),
        }]))
        .hiding_everywhere("mode")
        .hiding_everywhere("operations")
    );
    delete_registrations(&broker, &namespace, &[1]).await
}

/// Pins: Check that one can query context source registrations matching `EntityInfo` of `RegistrationInfo`
/// Clause: 5.10.2
/// Tenant: private (the case registers a Context Source by its propertyNames alone, which matches every entity type
/// (clause 5.12) and so every query of the shared tenant that names no attributes; clause 4.14 keeps each tenant's
/// registrations its own)
/// Changed: the query's idPattern is prefixed with the test's namespace (`urn:ngsi-ld:OffStreetParking:<ns>-`).
/// Changed: the teardown delete is held to the 204 a deleted registration answers, not only to a status below 400.
#[gtest]
#[tokio::test]
async fn e037_05_02_registration_without_entity_info() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let tenant = broker::private_tenant(&broker, &namespace).await?;
    let vocabulary = Vocabulary::register(&tenant, &namespace, &["name", "subCategory", "OffStreetParking"]).await?;
    let endpoint = csource_endpoint(&namespace, "my.csource.org:1026").await?;
    let registration = CSourceRegistration::builder(endpoint.as_str())
        .id(namespace.urn_text("ContextSourceRegistration", 1).parse::<RegistrationId>()?)
        .information([RegistrationInfo::builder().property_names(["name", "subCategory"]).build()?])
        .build()?;
    let _registration = create_registration(&tenant, &vocabulary, &registration).await?;

    let body = downtown_parkings(&tenant, &vocabulary, &namespace).await?;
    expect_that!(
        &body,
        matches_document(json!([{
            "id": namespace.urn_text("ContextSourceRegistration", 1),
            "type": "ContextSourceRegistration",
            "information": [{"propertyNames": ["name", "subCategory"]}],
            "endpoint": endpoint.as_str(),
        }]))
        .hiding_everywhere("mode")
        .hiding_everywhere("operations")
    );
    delete_registrations(&tenant, &namespace, &[1]).await
}
