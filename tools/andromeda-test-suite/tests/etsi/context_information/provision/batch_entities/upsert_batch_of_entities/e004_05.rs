//! ETSI `004_05`: a batch upsert whose third entity has no type is answered 207, with the two valid entities written and
//! the invalid one reported in error (clause 5.6.8).

use crate::{
    suite_keywords::{bare_building_json, delete_entities, expect_partial_batch, ld_json_body_client, partial_batch_answer, query_entities_json},
    suite_payloads::bare_building,
};
use andromeda_test_suite::{broker, comparison::matches_document, namespace::Namespace, vocabulary::Vocabulary};
use googletest::prelude::*;
use ngsi_ld::{
    http::{UncheckedDocument, operation::BatchUpsert},
    model::{Lossless, NgsiLdMediaType},
};
use serde_json::json;

/// Pins: Check that one can upsert a batch of two valid entities and one invalid entity
/// Clause: 5.6.8
#[gtest]
#[tokio::test]
async fn e004_05_01_upsert_a_batch_of_two_valid_entities_and_one_invalid_entity() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Building"]).await?;
    let client = ld_json_body_client(&broker, &vocabulary)?;
    let entities = [bare_building(&namespace, 1)?, bare_building(&namespace, 2)?, bare_building(&namespace, 3)?];
    // The third entity is sent without its type.
    let documents = entities
        .iter()
        .enumerate()
        .map(|(position, entity)| {
            let document = UncheckedDocument::from_entity(entity, Lossless::Normalized);
            if position == 2 { document.remove_member("type") } else { document }
        })
        .collect::<Vec<_>>();
    let request = client.unchecked(&BatchUpsert::builder().entities(&entities).build())?.documents(&documents);
    let result = partial_batch_answer(&client, request).await?;
    expect_partial_batch(
        &result,
        &[&namespace.urn("Building", 1)?, &namespace.urn("Building", 2)?],
        &namespace.urn("Building", 3)?,
    )?;

    let client = broker.with_context(vocabulary.context())?.with_accept(NgsiLdMediaType::JsonLd);
    let body = query_entities_json(&client, vec![namespace.urn("Building", 1)?, namespace.urn("Building", 2)?], "Building").await?;
    let context = vocabulary.url().as_str();
    expect_that!(
        &body,
        matches_document(json!([bare_building_json(&namespace, 1, context), bare_building_json(&namespace, 2, context)]))
    );
    delete_entities(&broker, &[&namespace.urn("Building", 1)?, &namespace.urn("Building", 2)?]).await;
    Ok(())
}
