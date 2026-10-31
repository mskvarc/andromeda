//! The detailed representation of every entity type the broker holds is listed, with the `@context` the request carries
//! or without one (clause 5.7.6).

use crate::{building_payloads::building_simple_attributes, entity_operations::create_entity_with_link, vehicle_entity_payloads::vehicle_parked};
use andromeda_test_suite::{
    broker,
    comparison::matches_document,
    namespace::Namespace,
    problem::answered,
    teardown::AnyAnswer,
    vocabulary::Vocabulary,
    wire::json_body,
};
use googletest::prelude::*;
use http::StatusCode;
use serde_json::{Value, json};

/// The terms of the case's `@context`.
const TERMS: [&str; 9] = [
    "Building",
    "name",
    "subCategory",
    "airQualityLevel",
    "almostFull",
    "Vehicle",
    "brandName",
    "isParked",
    "providedBy",
];

/// The detailed representations of the Building and Vehicle types with no `@context`: names are full IRIs.
fn expanded_details(namespace: &Namespace) -> Result<Value> {
    Ok(json!([
        {
            "id": namespace.expanded_term("Building")?,
            "type": "EntityType",
            "typeName": namespace.expanded_term("Building")?,
            "attributeNames": [
                namespace.expanded_term("name")?,
                namespace.expanded_term("subCategory")?,
                namespace.expanded_term("airQualityLevel")?,
                namespace.expanded_term("almostFull")?,
            ],
        },
        {
            "id": namespace.expanded_term("Vehicle")?,
            "type": "EntityType",
            "typeName": namespace.expanded_term("Vehicle")?,
            "attributeNames": [
                namespace.expanded_term("brandName")?,
                namespace.expanded_term("isParked")?,
            ],
        },
    ]))
}

/// The detailed representations of the Building and Vehicle types with their `@context`: names are compacted.
fn compacted_details(namespace: &Namespace) -> Result<Value> {
    Ok(json!([
        {
            "id": namespace.expanded_term("Building")?,
            "type": "EntityType",
            "typeName": "Building",
            "attributeNames": ["name", "subCategory", "airQualityLevel", "almostFull"],
        },
        {
            "id": namespace.expanded_term("Vehicle")?,
            "type": "EntityType",
            "typeName": "Vehicle",
            "attributeNames": ["brandName", "isParked"],
        },
    ]))
}

/// Pins: Check that one can retrieve a list with a detailed representation of NGSI-LD entity types
/// Clause: 5.7.6
/// Expected: isParked expands to the test vocabulary's IRI (clause 5.5.7)
/// Tenant: private (the response lists every entity type of the tenant)
#[gtest]
#[tokio::test]
async fn e023_01_01_without_json_ld_context() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let tenant = broker::private_tenant(&broker, &namespace).await?;
    let vocabulary = Vocabulary::register(&tenant, &namespace, &TERMS).await?;
    let building = building_simple_attributes(&namespace, 1)?;
    let vehicle = vehicle_parked(&namespace, 1)?;
    create_entity_with_link(&tenant, &vocabulary, &building).await?;
    create_entity_with_link(&tenant, &vocabulary, &vehicle).await?;
    let received = tenant.with_responses().entity_types().list_details().send().await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    expect_that!(&body, matches_document(expanded_details(&namespace)?));
    tenant.entities().delete(&building.id).send().await.any_answer();
    tenant.temporal().delete(&building.id).send().await.any_answer();
    tenant.entities().delete(&vehicle.id).send().await.any_answer();
    tenant.temporal().delete(&vehicle.id).send().await.any_answer();
    Ok(())
}

/// Pins: Check that one can retrieve a list with a detailed representation of NGSI-LD entity types
/// Clause: 5.7.6
/// Tenant: private (the response lists every entity type of the tenant)
#[gtest]
#[tokio::test]
async fn e023_01_02_with_json_ld_context() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let tenant = broker::private_tenant(&broker, &namespace).await?;
    let vocabulary = Vocabulary::register(&tenant, &namespace, &TERMS).await?;
    let building = building_simple_attributes(&namespace, 1)?;
    let vehicle = vehicle_parked(&namespace, 1)?;
    create_entity_with_link(&tenant, &vocabulary, &building).await?;
    create_entity_with_link(&tenant, &vocabulary, &vehicle).await?;
    let received = tenant
        .with_context(vocabulary.context())?
        .with_responses()
        .entity_types()
        .list_details()
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    expect_that!(&body, matches_document(compacted_details(&namespace)?));
    tenant.entities().delete(&building.id).send().await.any_answer();
    tenant.temporal().delete(&building.id).send().await.any_answer();
    tenant.entities().delete(&vehicle.id).send().await.any_answer();
    tenant.temporal().delete(&vehicle.id).send().await.any_answer();
    Ok(())
}
