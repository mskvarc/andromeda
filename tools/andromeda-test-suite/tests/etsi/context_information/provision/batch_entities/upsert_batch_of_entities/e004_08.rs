//! A batch upsert of existing entities with a new type replaces each entity with the types the batch carries (clauses
//! 4.16 and 5.6.8).

use crate::{
    batch_operations::expect_batch_done,
    building_documents::typed_building_json,
    building_payloads::{bare_building, building_of_types},
    entity_operations::{create_entity_with_context_string_any, delete_entities, ld_json_body_client, query_entities_json},
};
use andromeda_test_suite::{broker, comparison::matches_document, namespace::Namespace, vocabulary::Vocabulary};
use googletest::prelude::*;
use ngsi_ld::model::NgsiLdMediaType;
use serde_json::json;

/// Pins: Check that you can upsert a batch of existing entities with new types
/// Clause: 5.6.8, 4.16
#[gtest]
#[tokio::test]
async fn e004_08_01_entity_with_new_types() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Building", "Vehicle"]).await?;
    create_entity_with_context_string_any(&broker, &vocabulary, &bare_building(&namespace, 1)?).await?;
    create_entity_with_context_string_any(&broker, &vocabulary, &bare_building(&namespace, 2)?).await?;
    let entities = [
        building_of_types(&namespace, 1, ["Building", "Vehicle"])?,
        building_of_types(&namespace, 2, ["Building", "Vehicle"])?,
    ];
    let answer = ld_json_body_client(&broker, &vocabulary)?
        .with_responses()
        .entities()
        .batch_upsert(&entities)
        .send()
        .await;
    expect_batch_done(answer)?;

    let client = broker.with_context(vocabulary.context())?.with_accept(NgsiLdMediaType::JsonLd);
    let body = query_entities_json(&client, vec![namespace.urn("Building", 1)?, namespace.urn("Building", 2)?], "Building").await?;
    let context = vocabulary.url().as_str();
    expect_that!(
        &body,
        matches_document(json!([
            typed_building_json(&namespace, 1, &["Building", "Vehicle"], context),
            typed_building_json(&namespace, 2, &["Building", "Vehicle"], context)
        ]))
    );
    delete_entities(&broker, &[&namespace.urn("Building", 1)?, &namespace.urn("Building", 2)?]).await;
    Ok(())
}
