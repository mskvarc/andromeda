//! A batch naming one existing and one absent entity deletes the existing one and reports the absent one in error
//! (clause 5.6.10).

use crate::{
    batch_operations::{expect_partial_batch, partial_batch_outcome},
    building_payloads::{BUILDING_SIMPLE_TERMS, building_simple_attributes},
    entity_operations::{create_entity_with_context_string, query_entities_json},
};
use andromeda_test_suite::{broker, comparison::matches_document, namespace::Namespace, vocabulary::Vocabulary};
use googletest::prelude::*;
use ngsi_ld::http::headers::Accept;
use serde_json::json;

/// Pins: Check that one can delete a batch of non-existing and existing entities
/// Clause: 5.6.10
#[gtest]
#[tokio::test]
async fn e006_02_01_delete_a_batch_of_non_existing_and_existing_entities() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &BUILDING_SIMPLE_TERMS).await?;
    create_entity_with_context_string(&broker, &vocabulary, &building_simple_attributes(&namespace, 1)?).await?;
    let (existing, absent) = (namespace.urn("Building", 1)?, namespace.urn("Building", 2)?);
    let answer = broker.with_responses().entities().batch_delete(&[existing, absent]).send().await;
    let result = partial_batch_outcome(answer)?;
    expect_partial_batch(&result, &[&namespace.urn("Building", 1)?], &namespace.urn("Building", 2)?)?;
    let client = broker.with_context(vocabulary.context())?.with_accept(Accept::any());
    let body = query_entities_json(&client, vec![namespace.urn("Building", 1)?], "Building").await?;
    expect_that!(&body, matches_document(json!([])));
    Ok(())
}
