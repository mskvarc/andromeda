//! A scope appended to an entity is added beside its scopes when overwriting is disabled, and replaces them when
//! overwriting is enabled (clauses 5.6.3 and 4.18).

use crate::{
    building_payloads::company_unit_scope,
    entity_operations::{append_attributes, create_entity_with_link, delete_entities, retrieve_entity_json},
};
use andromeda_test_suite::{broker, comparison::matches_document, namespace::Namespace, vocabulary::Vocabulary};
use googletest::prelude::*;
use ngsi_ld::{
    http::operation::Overwrite,
    model::{Entity, Input, NgsiLdMediaType},
};
use serde_json::json;

/// The terms of the `@context` the cases register.
const TERMS: [&str; 1] = ["Building"];

/// The Building `urn:ngsi-ld:Vehicle:<ns>-1` of the setup, whose `scope` is `/Madrid/Gardens/ParqueNorte`.
fn building_with_parque_norte_scope(namespace: &Namespace) -> Result<Entity<Input>> {
    Ok(Entity::builder()
        .id(namespace.urn("Vehicle", 1)?)
        .entity_type("Building")
        .scope("/Madrid/Gardens/ParqueNorte")
        .build()?)
}

/// Pins: Check that scope is appended if overwrite is disabled
/// Clause: 5.6.3, 4.18
#[gtest]
#[tokio::test]
async fn e010_07_01_append_scope_to_an_entity_with_overwrite_disabled() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &TERMS).await?;
    let original = building_with_parque_norte_scope(&namespace)?;
    create_entity_with_link(&broker, &vocabulary, &original).await?;
    append_attributes(&broker, &original.id, &company_unit_scope()?, Overwrite::Keep).await?;
    let client = broker.with_context(vocabulary.context())?.with_accept(NgsiLdMediaType::Json);
    let body = retrieve_entity_json(&client, &original.id).await?;
    expect_that!(
        &body,
        matches_document(json!({
            "id": original.id.to_string(),
            "type": "Building",
            "scope": ["/Madrid/Gardens/ParqueNorte", "/CompanyA/OrganizationB/UnitC"],
        }))
    );
    delete_entities(&broker, &[&original.id]).await;
    Ok(())
}

/// Pins: Check that scope is replaced if overwrite is enabled
/// Clause: 5.6.3, 4.18
#[gtest]
#[tokio::test]
async fn e010_07_01_append_scope_to_an_entity_with_overwrite_enabled() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &TERMS).await?;
    let original = building_with_parque_norte_scope(&namespace)?;
    create_entity_with_link(&broker, &vocabulary, &original).await?;
    append_attributes(&broker, &original.id, &company_unit_scope()?, Overwrite::Replace).await?;
    let client = broker.with_context(vocabulary.context())?.with_accept(NgsiLdMediaType::Json);
    let body = retrieve_entity_json(&client, &original.id).await?;
    expect_that!(
        &body,
        matches_document(json!({
            "id": original.id.to_string(),
            "type": "Building",
            "scope": "/CompanyA/OrganizationB/UnitC",
        }))
    );
    delete_entities(&broker, &[&original.id]).await;
    Ok(())
}
