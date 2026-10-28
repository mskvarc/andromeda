//! ETSI `054_01`: an entity is replaced, and its `createdAt` system attribute keeps the value it had
//! (clauses 5.6.18 and 6.5.3.3).

use crate::{
    suite_keywords::{create_entity_with_link, delete_entities, non_empty_member},
    suite_payloads::{BUILDING_REPLACE_TERMS, building_simple_attributes, pisa_tower_located_at_pisa},
};
use andromeda_test_suite::{broker, comparison::matches_document, namespace::Namespace, problem::answered, vocabulary::Vocabulary, wire::json_body};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::http::{headers::Accept, operation::SystemAttributes};
use serde_json::json;

/// Pins: Check that one can replace an existing entity and that its createdAt Temporal Property remains unchanged
/// Clause: 5.6.18, 6.5.3.3
#[gtest]
#[tokio::test]
async fn e054_01_01_replace_an_existing_entity() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &BUILDING_REPLACE_TERMS).await?;
    let original = building_simple_attributes(&namespace, 1)?;
    create_entity_with_link(&broker, &vocabulary, &original).await?;
    let client = broker.with_context(vocabulary.context())?;
    let before = client
        .with_accept(Accept::any())
        .with_responses()
        .entities()
        .retrieve(&original.id)
        .system_attributes(SystemAttributes::Include)
        .send()
        .await;
    verify_that!(&before, answered(StatusCode::OK))?;
    let before = json_body(&before?)?;
    let created_at = before.pointer("/createdAt");
    verify_that!(created_at, some(anything()))?;
    let replacement = pisa_tower_located_at_pisa(&namespace, 1)?;
    let replaced = client.with_responses().entities().replace(&replacement).send().await;
    verify_that!(&replaced, answered(StatusCode::NO_CONTENT))?;
    expect_that!(replaced?.body.len(), eq(0));
    let after = client
        .with_accept(Accept::any())
        .with_responses()
        .entities()
        .retrieve(&original.id)
        .system_attributes(SystemAttributes::Include)
        .send()
        .await;
    verify_that!(&after, answered(StatusCode::OK))?;
    let after = json_body(&after?)?;
    expect_that!(
        &after,
        matches_document(json!({
            "id": original.id.to_string(),
            "type": "Building",
            "name": {"type": "Property", "value": "Pisa Tower"},
            "locatedAt": {"type": "Relationship", "object": namespace.urn_text("City", "Pisa")},
        }))
        .hiding_everywhere("@context")
        .hiding_everywhere("createdAt")
        .hiding_everywhere("modifiedAt")
    );
    expect_that!(after.pointer("/createdAt"), eq(created_at));
    expect_that!(non_empty_member(&after, "createdAt"), some(anything()));
    delete_entities(&broker, &[&original.id]).await;
    Ok(())
}
