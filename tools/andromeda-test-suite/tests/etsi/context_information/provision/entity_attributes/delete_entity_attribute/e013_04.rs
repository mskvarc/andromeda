//! The scope of an entity is deleted (clauses 5.6.5 and 4.18).

use crate::entity_operations::{create_entity_plain, delete_entities, retrieve_entity_json};
use andromeda_test_suite::{
    broker,
    comparison::matches_document,
    namespace::Namespace,
    problem::answered,
    wire::{content_type, request_to},
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::model::{APPLICATION_JSON, Entity, NgsiLdMediaType};
use serde_json::json;

/// Pins: Check that one can delete a scope from an entity
/// Clause: 5.6.5, 4.18
/// Vocabulary: none (the test references no @context)
#[gtest]
#[tokio::test]
async fn e013_04_delete_scope_from_an_entity() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let id = namespace.urn("Vehicle", 1)?;
    let entity = Entity::builder()
        .id(namespace.urn("Vehicle", 1)?)
        .entity_type("Building")
        .scope("/Madrid/Gardens/ParqueNorte")
        .build()?;
    create_entity_plain(&broker, &entity).await?;
    // `AttributeName` refuses `scope`, a name the core @context reserves, so the target is written as text.
    let deletion = request_to(&broker, Method::DELETE, &format!("entities/{id}/attrs/scope?deleteAll=false"))?;
    let deleted = broker.send_unchecked(content_type(deletion, &APPLICATION_JSON)?).await;
    verify_that!(&deleted, answered(StatusCode::NO_CONTENT))?;
    expect_that!(deleted?.body().len(), eq(0));

    let body = retrieve_entity_json(&broker.with_accept(NgsiLdMediaType::Json), &id).await?;
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
