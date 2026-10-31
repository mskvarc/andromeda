//! The attributes the broker holds are listed, with the `@context` the request carries or without one (clause 5.7.8).

use crate::{building_payloads::building_simple_attributes, entity_operations::create_entity_with_link};
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
const TERMS: [&str; 5] = ["Building", "name", "subCategory", "airQualityLevel", "almostFull"];

/// Pins: Check that one can retrieve a list of NGSI-LD attributes
/// Clause: 5.7.8
/// Expected: the list's id is the broker's choice, so the member count is bounded at 3 (id, type, attributeList) rather
/// than fixed
/// Tenant: private (the response lists every attribute of the tenant)
#[gtest]
#[tokio::test]
async fn e025_01_01_without_json_ld_context() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let tenant = broker::private_tenant(&broker, &namespace).await?;
    let vocabulary = Vocabulary::register(&tenant, &namespace, &TERMS).await?;
    let building = building_simple_attributes(&namespace, 1)?;
    create_entity_with_link(&tenant, &vocabulary, &building).await?;
    let received = tenant.with_responses().attributes().list().details(SummaryDetails::Explicit).send().await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    expect_that!(body.as_object().map(Map::len), some(le(3)));
    expect_that!(
        &body,
        matches_document(json!({
            "type": "AttributeList",
            "attributeList": [
                namespace.expanded_term("name")?,
                namespace.expanded_term("subCategory")?,
                namespace.expanded_term("airQualityLevel")?,
                namespace.expanded_term("almostFull")?,
            ],
        }))
        .hiding_everywhere("id")
    );
    tenant.entities().delete(&building.id).send().await.any_answer();
    tenant.temporal().delete(&building.id).send().await.any_answer();
    Ok(())
}

/// Pins: Check that one can retrieve a list of NGSI-LD attributes
/// Clause: 5.7.8
/// Expected: the list's id is the broker's choice, so the member count is bounded at 3 (id, type, attributeList) rather
/// than fixed
/// Tenant: private (the response lists every attribute of the tenant)
#[gtest]
#[tokio::test]
async fn e025_01_02_with_json_ld_context() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let tenant = broker::private_tenant(&broker, &namespace).await?;
    let vocabulary = Vocabulary::register(&tenant, &namespace, &TERMS).await?;
    let building = building_simple_attributes(&namespace, 1)?;
    create_entity_with_link(&tenant, &vocabulary, &building).await?;
    let received = tenant
        .with_context(vocabulary.context())?
        .with_responses()
        .attributes()
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
            "type": "AttributeList",
            "attributeList": ["name", "subCategory", "airQualityLevel", "almostFull"],
        }))
        .hiding_everywhere("id")
    );
    tenant.entities().delete(&building.id).send().await.any_answer();
    tenant.temporal().delete(&building.id).send().await.any_answer();
    Ok(())
}
