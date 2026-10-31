//! Context source registrations are queried by a list of entity types or by a list of attribute names alone (clause
//! 5.10.2).

use crate::{
    registration_operations::{building_registration_json, create_discovery_registrations, delete_registrations},
    registration_payloads::VEHICLE_PARKING_BUILDING_TERMS,
};
use andromeda_test_suite::{broker, comparison::matches_document, namespace::Namespace, problem::answered, wire::json_body};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::model::AttributeName;
use serde_json::json;

/// Pins: Check that one can query context source registrations if at least one of list of Entity Types or list of
/// Attribute names is present
/// Clause: 5.10.2
/// Isolation: each idPattern is prefixed with the test's namespace (`urn:ngsi-ld:OffStreetParking:<ns>-`), so it
/// matches only the test's own entities
#[gtest]
#[tokio::test]
async fn e037_01_01_with_list_of_entity_types() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let (vocabulary, endpoint, _registrations) = create_discovery_registrations(&broker, &namespace, &VEHICLE_PARKING_BUILDING_TERMS).await?;
    let received = broker
        .with_context(vocabulary.context())?
        .with_responses()
        .registrations()
        .query()
        .entity_type("Building")
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    expect_that!(
        &body,
        matches_document(json!([building_registration_json(&namespace, 2, &endpoint)]))
            .hiding_everywhere("mode")
            .hiding_everywhere("operations")
    );
    delete_registrations(&broker, &namespace, &[1, 2]).await
}

/// Pins: Check that one can query context source registrations if at least one of list of Entity Types or list of
/// Attribute names is present
/// Clause: 5.10.2
/// Tenant: private (the case queries registrations by a list of attribute names alone, with no entity type; clause
/// 5.10.2.4 matches every registration of the tenant whose `RegistrationInfo` names that attribute or names no attribute
/// at all ("If no Properties or Relationships are specified in the `RegistrationInfo`, the Attribute names are
/// considered matching"), so in the shared tenant other files' registrations of entities alone are listed beside the
/// case's own and the exact count fails; clause 4.14 keeps each tenant's registrations its own)
/// Isolation: each idPattern is prefixed with the test's namespace (`urn:ngsi-ld:OffStreetParking:<ns>-`), so it
/// matches only the test's own entities
#[gtest]
#[tokio::test]
async fn e037_01_02_with_list_of_attribute_names() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let tenant = broker::private_tenant(&broker, &namespace).await?;
    let (vocabulary, endpoint, _registrations) = create_discovery_registrations(&tenant, &namespace, &VEHICLE_PARKING_BUILDING_TERMS).await?;
    let received = tenant
        .with_context(vocabulary.context())?
        .with_responses()
        .registrations()
        .query()
        .attrs(vec!["name".parse::<AttributeName>()?])
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    expect_that!(
        &body,
        matches_document(json!([building_registration_json(&namespace, 2, &endpoint)]))
            .hiding_everywhere("mode")
            .hiding_everywhere("operations")
    );
    delete_registrations(&tenant, &namespace, &[1, 2]).await
}
