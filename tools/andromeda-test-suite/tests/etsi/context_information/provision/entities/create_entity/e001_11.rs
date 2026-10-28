//! ETSI `001_11`: an entity with one scope, and one with many scopes, is created and retrieved with its scope
//! (clause 4.18).

use crate::{
    suite_keywords::{create_entity_plain, delete_entities, expect_created, retrieve_entity_json},
    suite_payloads::{building_with_many_scopes, building_with_one_scope},
};
use andromeda_test_suite::{broker, comparison::matches_document, namespace::Namespace};
use googletest::prelude::*;
use ngsi_ld::model::NgsiLdMediaType;
use serde_json::json;

/// Pins: Check that one can create an entity with one or more scopes
/// Clause: 4.18
/// Changed: Check Response Headers Containing URI set to checks Location only when the response header dictionary
/// holds a key spelled exactly `Location`, which a broker sending a lower-case `location` (valid per RFC 9110, and what
/// Andromeda sends) never satisfies; Hurl matches header names without regard to case and cannot make an assertion
/// conditional, so the port requires a Location header that contains the entity id
/// Vocabulary: none (the ETSI case references no @context)
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
/// Changed: Check Response Headers Containing URI set to checks Location only when the response header dictionary
/// holds a key spelled exactly `Location`, which a broker sending a lower-case `location` (valid per RFC 9110, and what
/// Andromeda sends) never satisfies; Hurl matches header names without regard to case and cannot make an assertion
/// conditional, so the port requires a Location header that contains the entity id
/// Vocabulary: none (the ETSI case references no @context)
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
