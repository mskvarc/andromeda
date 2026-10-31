//! An entity with one scope, and one with many scopes, is created and retrieved with its scope (clause 4.18).

use crate::{
    building_payloads::{building_with_many_scopes, building_with_one_scope},
    entity_operations::{create_entity_plain, delete_entities, expect_created, retrieve_entity_json},
};
use andromeda_test_suite::{broker, comparison::matches_document, namespace::Namespace};
use googletest::prelude::*;
use ngsi_ld::model::NgsiLdMediaType;
use serde_json::json;

/// Pins: Check that one can create an entity with one or more scopes
/// Clause: 4.18
/// Vocabulary: none (the test references no @context)
#[gtest]
#[tokio::test]
async fn e001_11_01_entity_with_one_scope() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let entity = building_with_one_scope(&namespace)?;
    let created = create_entity_plain(&broker, &entity).await?;
    expect_created(&created, &entity.id);
    let body = retrieve_entity_json(&broker.with_accept(NgsiLdMediaType::Json), &entity.id).await?;
    expect_that!(
        &body,
        matches_document(json!({
            "id": entity.id.to_string(),
            "type": "Building",
            "scope": "/Madrid/Gardens/ParqueNorte",
        }))
    );
    delete_entities(&broker, &[&entity.id]).await;
    Ok(())
}

/// Pins: Check that one can create an entity with one or more scopes
/// Clause: 4.18
/// Vocabulary: none (the test references no @context)
#[gtest]
#[tokio::test]
async fn e001_11_02_entity_with_many_scopes() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let entity = building_with_many_scopes(&namespace)?;
    let created = create_entity_plain(&broker, &entity).await?;
    expect_created(&created, &entity.id);
    let body = retrieve_entity_json(&broker.with_accept(NgsiLdMediaType::Json), &entity.id).await?;
    expect_that!(
        &body,
        matches_document(json!({
            "id": entity.id.to_string(),
            "type": "Building",
            "scope": ["/Madrid/Gardens/ParqueNorte", "/CompanyA/OrganizationB/UnitC"],
        }))
    );
    delete_entities(&broker, &[&entity.id]).await;
    Ok(())
}
