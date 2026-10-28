//! ETSI `033_01`: a context source registration is created without a mode and operations, with an `exclusive` mode
//! and operations, with a location or with an expiration date, and retrieved with the members it was created with
//! (clause 5.9.2).

use crate::{
    suite_keywords::{create_registration, csource_endpoint},
    suite_payloads::{VEHICLE_PARKING_TERMS, a456_information, offstreet_pattern, point, vehicle_parking_registration},
};
use andromeda_test_suite::{
    broker::{self, BrokerClient},
    cleanup::Cleanup,
    comparison::matches_document,
    namespace::Namespace,
    vocabulary::Vocabulary,
    wire::json_body,
};
use googletest::prelude::*;
use ngsi_ld::{
    model::{DateTime, EntityTypeName, Input, NgsiLdMediaType},
    registration::{
        CSourceRegistration,
        DistributedOperation,
        EntityInfo,
        OperationGroup,
        RegistrationId,
        RegistrationInfo,
        RegistrationMode,
        SupportedOperation,
    },
};
use serde_json::{Value, json};
use url::Url;

/// The case's setup: registers `terms` and `registration` in their context (201, the answer's `Location` naming its
/// id), and answers the vocabulary, the registration's identifier and the cleanup that deletes the registration.
async fn created_registration(
    broker: &BrokerClient,
    namespace: &Namespace,
    terms: &[&str],
    registration: &CSourceRegistration<Input>,
) -> Result<(Vocabulary, RegistrationId, Cleanup)> {
    let vocabulary = Vocabulary::register(broker, namespace, terms).await?;
    let cleanup = create_registration(broker, &vocabulary, registration).await?;
    Ok((vocabulary, registration.id().cloned().or_fail()?, cleanup))
}

/// Retrieves `id` as `application/ld+json` in `vocabulary`'s context, answering the body.
async fn retrieved(broker: &BrokerClient, vocabulary: &Vocabulary, id: &RegistrationId) -> Result<Value> {
    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(NgsiLdMediaType::JsonLd)
        .with_responses()
        .registrations()
        .retrieve(id)
        .send()
        .await;
    Ok(json_body(&received?)?)
}

/// The registration's endpoint, the case's context source.
async fn case_source(namespace: &Namespace) -> Result<Url> {
    csource_endpoint(namespace, "my.csource.org:1026").await
}

/// Pins: Check that one can create a context source registration with specific ID and expiration date
/// Clause: 5.9.2
/// Changed: Check Response Headers Containing URI set to checks Location only when the response header dictionary holds
/// a key spelled exactly `Location`, which a broker sending a lower-case `location` (valid per RFC 9110, and what
/// Andromeda sends) never satisfies; Hurl matches header names without regard to case and cannot make an assertion
/// conditional, so the port requires a Location header that contains the registration id
/// Changed: each idPattern is prefixed with the test's namespace (`urn:ngsi-ld:OffStreetParking:<ns>-`), so it matches
/// only the test's own entities.
/// Changed: the teardown delete is held to the 204 a deleted registration answers, not only to a status below 400.
#[gtest]
#[tokio::test]
async fn e033_01_01_create_context_source_registration_without_mode_and_operation() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let endpoint = case_source(&namespace).await?;
    let registration = vehicle_parking_registration(&namespace, &endpoint, namespace.urn("Vehicle", "A456")?, None)?;
    let (vocabulary, id, _registration) = created_registration(&broker, &namespace, &VEHICLE_PARKING_TERMS, &registration).await?;
    let body = retrieved(&broker, &vocabulary, &id).await?;
    let downtown = offstreet_pattern(&namespace, ".*downtown$")?;
    let forty_seven = offstreet_pattern(&namespace, ".*47$")?;
    expect_that!(
        &body,
        matches_document(json!({
            "id": id.to_string(),
            "type": "ContextSourceRegistration",
            "information": [
                {
                    "entities": [{"id": namespace.urn_text("Vehicle", "A456"), "type": "Vehicle"}],
                    "propertyNames": ["brandName", "speed"],
                    "relationshipNames": ["isParked"]
                },
                {
                    "entities": [
                        {"idPattern": downtown.as_ref(), "type": "OffStreetParking"},
                        {"idPattern": forty_seven.as_ref(), "type": "OffStreetParking"}
                    ],
                    "propertyNames": ["availableSpotNumber", "totalSpotNumber"],
                    "relationshipNames": ["isNextToBuilding"]
                }
            ],
            "endpoint": endpoint.as_str(),
            "@context": vocabulary.url().as_str(),
        }))
        .hiding_everywhere("status")
        .hiding_everywhere("mode")
        .hiding_everywhere("operations")
    );
    broker.registrations().delete(&id).send().await?;
    Ok(())
}

/// Pins: Check that one can create a context source registration with specific ID and expiration date
/// Clause: 5.9.2
/// Corrected: `fix(033_01_02)`: exclusive registration names entities by id (4.3.6.3)
/// Changed: Check Response Headers Containing URI set to checks Location only when the response header dictionary holds
/// a key spelled exactly `Location`, which a broker sending a lower-case `location` (valid per RFC 9110, and what
/// Andromeda sends) never satisfies; Hurl matches header names without regard to case and cannot make an assertion
/// conditional, so the port requires a Location header that contains the registration id
/// Changed: the teardown delete is held to the 204 a deleted registration answers, not only to a status below 400.
#[gtest]
#[tokio::test]
async fn e033_01_02_create_context_source_registration_with_non_default_mode_and_operations() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let endpoint = case_source(&namespace).await?;
    let parking = RegistrationInfo::builder()
        .entities([
            EntityInfo::new("OffStreetParking".parse::<EntityTypeName>()?).with_id(namespace.urn("OffStreetParking", "downtown")?),
            EntityInfo::new("OffStreetParking".parse::<EntityTypeName>()?).with_id(namespace.urn("OffStreetParking", "p47")?),
        ])
        .property_names(["availableSpotNumber", "totalSpotNumber"])
        .relationship_names(["isNextToBuilding"])
        .build()?;
    let registration = CSourceRegistration::builder(endpoint.as_str())
        .id(namespace.urn_text("ContextSourceRegistration", 1).parse::<RegistrationId>()?)
        .information([a456_information(namespace.urn("Vehicle", "A456")?)?, parking])
        .mode(RegistrationMode::Exclusive)
        .operations([
            SupportedOperation::from(DistributedOperation::QueryEntity),
            SupportedOperation::from(DistributedOperation::QueryTemporal),
            SupportedOperation::from(OperationGroup::UpdateOps),
        ])
        .build()?;
    let (vocabulary, id, _registration) = created_registration(&broker, &namespace, &VEHICLE_PARKING_TERMS, &registration).await?;
    let body = retrieved(&broker, &vocabulary, &id).await?;
    expect_that!(
        &body,
        matches_document(json!({
            "id": id.to_string(),
            "type": "ContextSourceRegistration",
            "information": [
                {
                    "entities": [{"id": namespace.urn_text("Vehicle", "A456"), "type": "Vehicle"}],
                    "propertyNames": ["brandName", "speed"],
                    "relationshipNames": ["isParked"]
                },
                {
                    "entities": [
                        {"id": namespace.urn_text("OffStreetParking", "downtown"), "type": "OffStreetParking"},
                        {"id": namespace.urn_text("OffStreetParking", "p47"), "type": "OffStreetParking"}
                    ],
                    "propertyNames": ["availableSpotNumber", "totalSpotNumber"],
                    "relationshipNames": ["isNextToBuilding"]
                }
            ],
            "mode": "exclusive",
            "operations": ["queryEntity", "queryTemporal", "updateOps"],
            "endpoint": endpoint.as_str(),
            "@context": vocabulary.url().as_str(),
        }))
        .hiding_everywhere("status")
    );
    broker.registrations().delete(&id).send().await?;
    Ok(())
}

/// Pins: Check that one can create a context source registration with specific ID and expiration date
/// Clause: 5.9.2
/// Changed: Check Response Headers Containing URI set to checks Location only when the response header dictionary holds
/// a key spelled exactly `Location`, which a broker sending a lower-case `location` (valid per RFC 9110, and what
/// Andromeda sends) never satisfies; Hurl matches header names without regard to case and cannot make an assertion
/// conditional, so the port requires a Location header that contains the registration id
/// Changed: the teardown delete is held to the 204 a deleted registration answers, not only to a status below 400.
#[gtest]
#[tokio::test]
async fn e033_01_03_create_context_source_registration_with_location() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let endpoint = case_source(&namespace).await?;
    let registration = CSourceRegistration::builder(endpoint.as_str())
        .id(namespace.urn_text("ContextSourceRegistration", 1).parse::<RegistrationId>()?)
        .information([RegistrationInfo::builder().entities(["Building"]).build()?])
        .location(point(-8.521, 41.2)?)
        .build()?;
    let (vocabulary, id, _registration) = created_registration(&broker, &namespace, &["Building"], &registration).await?;
    let body = retrieved(&broker, &vocabulary, &id).await?;
    expect_that!(
        &body,
        matches_document(json!({
            "id": id.to_string(),
            "type": "ContextSourceRegistration",
            "information": [{"entities": [{"type": "Building"}]}],
            "location": {"type": "Point", "coordinates": [-8.521, 41.2]},
            "endpoint": endpoint.as_str(),
            "@context": vocabulary.url().as_str(),
        }))
        .hiding_everywhere("status")
        .hiding_everywhere("mode")
        .hiding_everywhere("operations")
    );
    broker.registrations().delete(&id).send().await?;
    Ok(())
}

/// Pins: Check that one can create a context source registration with specific ID and expiration date
/// Clause: 5.9.2
/// Changed: Check Response Headers Containing URI set to checks Location only when the response header dictionary holds
/// a key spelled exactly `Location`, which a broker sending a lower-case `location` (valid per RFC 9110, and what
/// Andromeda sends) never satisfies; Hurl matches header names without regard to case and cannot make an assertion
/// conditional, so the port requires a Location header that contains the registration id
/// Changed: each idPattern is prefixed with the test's namespace (`urn:ngsi-ld:OffStreetParking:<ns>-`), so it matches
/// only the test's own entities.
/// Changed: the teardown delete is held to the 204 a deleted registration answers, not only to a status below 400.
#[gtest]
#[tokio::test]
async fn e033_01_04_create_context_source_registration_with_specific_date_expiration_date() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let endpoint = case_source(&namespace).await?;
    let expires_at = "2035-11-29T14:53:15Z".parse::<DateTime>()?;
    let registration = vehicle_parking_registration(&namespace, &endpoint, namespace.urn("Vehicle", "A456")?, Some(expires_at))?;
    let (vocabulary, id, _registration) = created_registration(&broker, &namespace, &VEHICLE_PARKING_TERMS, &registration).await?;
    let body = retrieved(&broker, &vocabulary, &id).await?;
    let downtown = offstreet_pattern(&namespace, ".*downtown$")?;
    let forty_seven = offstreet_pattern(&namespace, ".*47$")?;
    expect_that!(
        &body,
        matches_document(json!({
            "id": id.to_string(),
            "type": "ContextSourceRegistration",
            "information": [
                {
                    "entities": [{"id": namespace.urn_text("Vehicle", "A456"), "type": "Vehicle"}],
                    "propertyNames": ["brandName", "speed"],
                    "relationshipNames": ["isParked"]
                },
                {
                    "entities": [
                        {"idPattern": downtown.as_ref(), "type": "OffStreetParking"},
                        {"idPattern": forty_seven.as_ref(), "type": "OffStreetParking"}
                    ],
                    "propertyNames": ["availableSpotNumber", "totalSpotNumber"],
                    "relationshipNames": ["isNextToBuilding"]
                }
            ],
            "endpoint": endpoint.as_str(),
            "expiresAt": "2035-11-29T14:53:15Z",
            "@context": vocabulary.url().as_str(),
        }))
        .hiding_everywhere("status")
        .hiding_everywhere("mode")
        .hiding_everywhere("operations")
    );
    broker.registrations().delete(&id).send().await?;
    Ok(())
}
