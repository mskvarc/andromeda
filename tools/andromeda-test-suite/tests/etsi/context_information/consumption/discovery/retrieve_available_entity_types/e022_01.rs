//! The entity types the broker holds are listed, with the `@context` the request carries or without one (clause 5.7.5).

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
use ngsi_ld::http::operation::SummaryDetails;
use serde_json::{Map, json};

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

/// Pins: Check that one can retrieve a list of NGSI-LD entity types
/// Clause: 5.7.5
/// Expected: the list's id is the broker's choice, so the member count is bounded at 3 (id, type, typeList) rather than
/// fixed
/// Tenant: private (the response lists every entity type of the tenant)
#[gtest]
#[tokio::test]
async fn e022_01_01_without_json_ld_context() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let tenant = broker::private_tenant(&broker, &namespace).await?;
    let vocabulary = Vocabulary::register(&tenant, &namespace, &TERMS).await?;
    let building = building_simple_attributes(&namespace, 1)?;
    let vehicle = vehicle_parked(&namespace, 1)?;
    create_entity_with_link(&tenant, &vocabulary, &building).await?;
    create_entity_with_link(&tenant, &vocabulary, &vehicle).await?;
    let received = tenant.with_responses().entity_types().list().details(SummaryDetails::Explicit).send().await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    expect_that!(body.as_object().map(Map::len), some(le(3)));
    expect_that!(
        &body,
        matches_document(json!({
            "type": "EntityTypeList",
            "typeList": [namespace.expanded_term("Building")?, namespace.expanded_term("Vehicle")?],
        }))
        .hiding_everywhere("id")
    );
    tenant.entities().delete(&building.id).send().await.any_answer();
    tenant.temporal().delete(&building.id).send().await.any_answer();
    tenant.entities().delete(&vehicle.id).send().await.any_answer();
    tenant.temporal().delete(&vehicle.id).send().await.any_answer();
    Ok(())
}

/// Pins: Check that one can retrieve a list of NGSI-LD entity types
/// Clause: 5.7.5
/// Expected: the list's id is the broker's choice, so the member count is bounded at 3 (id, type, typeList) rather than
/// fixed
/// Tenant: private (the response lists every entity type of the tenant)
#[gtest]
#[tokio::test]
async fn e022_01_02_with_json_ld_context() -> Result<()> {
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
        .list()
        .details(SummaryDetails::Explicit)
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    expect_that!(body.as_object().map(Map::len), some(le(3)));
    expect_that!(
        &body,
        matches_document(json!({
            "type": "EntityTypeList",
            "typeList": ["Building", "Vehicle"],
        }))
        .hiding_everywhere("id")
    );
    tenant.entities().delete(&building.id).send().await.any_answer();
    tenant.temporal().delete(&building.id).send().await.any_answer();
    tenant.entities().delete(&vehicle.id).send().await.any_answer();
    tenant.temporal().delete(&vehicle.id).send().await.any_answer();
    Ok(())
}
