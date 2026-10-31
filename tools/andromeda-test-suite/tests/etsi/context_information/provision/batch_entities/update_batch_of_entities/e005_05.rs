//! A batch update that names a new type for two entities adds the type to each of them (clauses 5.6.9 and 4.16).

use crate::{
    batch_operations::expect_batch_done,
    building_payloads::bare_building,
    entity_operations::{create_entity_with_context_string, delete_entities, ld_json_body_client, query_entities_json},
};
use andromeda_test_suite::{broker, comparison::matches_document, namespace::Namespace, vocabulary::Vocabulary};
use googletest::prelude::*;
use ngsi_ld::model::{EntityFragment, NgsiLdMediaType};
use serde_json::json;

/// Pins: Check that you can update types of entities in a batch update operation
/// Clause: 5.6.9, 4.16
#[gtest]
#[tokio::test]
async fn e005_05_01_entity_with_new_type() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Vehicle", "Building"]).await?;
    create_entity_with_context_string(&broker, &vocabulary, &bare_building(&namespace, 1)?).await?;
    create_entity_with_context_string(&broker, &vocabulary, &bare_building(&namespace, 2)?).await?;
    let fragments = [
        EntityFragment::for_entity(namespace.urn("Building", 1)?, "Vehicle").build()?,
        EntityFragment::for_entity(namespace.urn("Building", 2)?, "Vehicle").build()?,
    ];
    let answer = ld_json_body_client(&broker, &vocabulary)?
        .with_responses()
        .entities()
        .batch_update(&fragments)
        .send()
        .await;
    expect_batch_done(answer)?;
    let client = broker.with_context(vocabulary.context())?.with_accept(NgsiLdMediaType::JsonLd);
    let body = query_entities_json(&client, vec![namespace.urn("Building", 1)?, namespace.urn("Building", 2)?], "Building").await?;
    let context = vocabulary.url().as_str();
    expect_that!(
        &body,
        matches_document(json!([
            {"id": namespace.urn_text("Building", 1), "type": ["Building", "Vehicle"], "@context": context},
            {"id": namespace.urn_text("Building", 2), "type": ["Building", "Vehicle"], "@context": context},
        ]))
    );
    delete_entities(&broker, &[&namespace.urn("Building", 1)?, &namespace.urn("Building", 2)?]).await;
    Ok(())
}
