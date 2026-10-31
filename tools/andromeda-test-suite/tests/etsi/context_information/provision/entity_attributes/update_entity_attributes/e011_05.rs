//! An update replaces the scope of an entity that already has one, and adds no scope to an entity that has none
//! (clauses 5.6.2 and 4.18).

use crate::{
    building_payloads::{company_unit_scope, vehicle_id_building, vehicle_id_building_with_scope},
    entity_operations::{create_entity_with_link, delete_entities, retrieve_entity_json, update_attributes},
};
use andromeda_test_suite::{broker, comparison::matches_document, namespace::Namespace, problem::answered, vocabulary::Vocabulary};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::model::NgsiLdMediaType;
use serde_json::json;

/// The terms of the `@context` the cases register.
const TERMS: [&str; 1] = ["Building"];

/// Pins: Check that scope is replaced if entity already has a scope
/// Clause: 5.6.2, 4.18
#[gtest]
#[tokio::test]
async fn e011_05_01_update_scope_to_an_entity_already_having_a_scope() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &TERMS).await?;
    let id = namespace.urn("Vehicle", 1)?;
    create_entity_with_link(&broker, &vocabulary, &vehicle_id_building_with_scope(&namespace)?).await?;
    update_attributes(&broker, &id, &company_unit_scope()?).await?;
    let client = broker.with_context(vocabulary.context())?.with_accept(NgsiLdMediaType::Json);
    let body = retrieve_entity_json(&client, &id).await?;
    expect_that!(
        &body,
        matches_document(json!({
            "id": id.to_string(),
            "type": "Building",
            "scope": "/CompanyA/OrganizationB/UnitC",
        }))
    );
    delete_entities(&broker, &[&id]).await;
    Ok(())
}

/// Pins: Check that scope is not added if entity does not already have a scope
/// Clause: 5.6.2, 4.18
#[gtest]
#[tokio::test]
async fn e011_05_02_update_scope_to_an_entity_not_having_a_scope() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &TERMS).await?;
    let id = namespace.urn("Vehicle", 1)?;
    create_entity_with_link(&broker, &vocabulary, &vehicle_id_building(&namespace)?).await?;
    let fragment = company_unit_scope()?;
    let answer = broker.with_responses().entities().update_attributes(&id, &fragment).send().await;
    expect_that!(&answer, answered(StatusCode::MULTI_STATUS));
    let client = broker.with_context(vocabulary.context())?.with_accept(NgsiLdMediaType::Json);
    let body = retrieve_entity_json(&client, &id).await?;
    expect_that!(
        &body,
        matches_document(json!({
            "id": id.to_string(),
            "type": "Building",
        }))
    );
    delete_entities(&broker, &[&id]).await;
    Ok(())
}
