//! A batch of two valid entities and one invalid entity is partly created, and the broker names the entity in error
//! (clause 5.6.7).

use crate::{
    batch_operations::{batch_create_partial, expect_partial_batch},
    building_payloads::bare_building,
    entity_operations::{create_entity_with_context_string, delete_entities, ld_json_body_client, query_entities_json},
};
use andromeda_test_suite::{broker, comparison::matches_document, namespace::Namespace, vocabulary::Vocabulary};
use googletest::prelude::*;
use ngsi_ld::model::NgsiLdMediaType;
use serde_json::json;

/// Pins: Check that one can create a batch of two valid entities and one invalid entity
/// Clause: 5.6.7
#[gtest]
#[tokio::test]
async fn e003_02_01_create_a_batch_of_two_valid_entities_and_one_invalid_entity() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Building"]).await?;
    create_entity_with_context_string(&broker, &vocabulary, &bare_building(&namespace, 1)?).await?;
    let entities = [bare_building(&namespace, 2)?, bare_building(&namespace, 3)?, bare_building(&namespace, 1)?];
    let (first, second, existing) = (namespace.urn("Building", 2)?, namespace.urn("Building", 3)?, namespace.urn("Building", 1)?);
    let result = batch_create_partial(&ld_json_body_client(&broker, &vocabulary)?, &entities).await?;
    expect_partial_batch(&result, &[&first, &second], &existing)?;
    let client = broker.with_context(vocabulary.context())?.with_accept(NgsiLdMediaType::JsonLd);
    let body = query_entities_json(&client, vec![namespace.urn("Building", 2)?, namespace.urn("Building", 3)?], "Building").await?;
    expect_that!(
        &body,
        matches_document(json!([
            {"id": namespace.urn_text("Building", 2), "type": "Building", "@context": vocabulary.url().as_str()},
            {"id": namespace.urn_text("Building", 3), "type": "Building", "@context": vocabulary.url().as_str()},
        ]))
    );
    delete_entities(&broker, &[&second, &first, &existing]).await;
    Ok(())
}
