//! ETSI `026_01`: the detailed representation of every attribute the broker holds is listed, with the `@context` the
//! request carries or without one (clause 5.7.9).

use crate::{suite_keywords::create_entity_with_link, suite_payloads::building_simple_attributes};
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
const TERMS: [&str; 5] = ["Building", "name", "subCategory", "airQualityLevel", "almostFull"];

/// The detail of the attribute `term` of the Building type with no `@context`: its names are full IRIs.
fn expanded_detail(namespace: &Namespace, term: &str) -> Result<Value> {
    let attribute = namespace.expanded_term(term)?;
    Ok(json!({
        "id": &attribute,
        "type": "Attribute",
        "attributeName": &attribute,
        "typeNames": [namespace.expanded_term("Building")?],
    }))
}

/// The detail of the attribute `term` of the Building type with its `@context`: its names are compacted.
fn compacted_detail(namespace: &Namespace, term: &str) -> Result<Value> {
    Ok(json!({
        "id": namespace.expanded_term(term)?,
        "type": "Attribute",
        "attributeName": term,
        "typeNames": ["Building"],
    }))
}

/// Pins: Check that one can retrieve a list with a detailed representation of NGSI-LD attributes
/// Clause: 5.7.9
/// Tenant: private (the response lists every attribute of the tenant)
#[gtest]
#[tokio::test]
async fn e026_01_01_without_json_ld_context() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let tenant = broker::private_tenant(&broker, &namespace).await?;
    let vocabulary = Vocabulary::register(&tenant, &namespace, &TERMS).await?;
    let building = building_simple_attributes(&namespace, 1)?;
    create_entity_with_link(&tenant, &vocabulary, &building).await?;
    let received = tenant.with_responses().attributes().list_details().send().await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    expect_that!(
        &body,
        matches_document(json!([
            expanded_detail(&namespace, "name")?,
            expanded_detail(&namespace, "subCategory")?,
            expanded_detail(&namespace, "airQualityLevel")?,
            expanded_detail(&namespace, "almostFull")?,
        ]))
    );
    tenant.entities().delete(&building.id).send().await.any_answer();
    tenant.temporal().delete(&building.id).send().await.any_answer();
    Ok(())
}

/// Pins: Check that one can retrieve a list with a detailed representation of NGSI-LD attributes
/// Clause: 5.7.9
/// Tenant: private (the response lists every attribute of the tenant)
#[gtest]
#[tokio::test]
async fn e026_01_02_with_json_ld_context() -> Result<()> {
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
        .list_details()
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    expect_that!(
        &body,
        matches_document(json!([
            compacted_detail(&namespace, "name")?,
            compacted_detail(&namespace, "subCategory")?,
            compacted_detail(&namespace, "airQualityLevel")?,
            compacted_detail(&namespace, "almostFull")?,
        ]))
    );
    tenant.entities().delete(&building.id).send().await.any_answer();
    tenant.temporal().delete(&building.id).send().await.any_answer();
    Ok(())
}
