//! ETSI `006_04`: a batch naming the same existing entity twice deletes it once and reports the repeat in error
//! (clauses 5.5.11 and 5.6.10).

use crate::{
    suite_keywords::{create_entity_with_context_string, expect_partial_batch, partial_batch_outcome},
    suite_payloads::{BUILDING_SIMPLE_TERMS, building_simple_attributes},
};
use andromeda_test_suite::{broker, namespace::Namespace, vocabulary::Vocabulary};
use googletest::prelude::*;

/// Pins: Check that one can delete a batch entities with the same id
/// Clause: 5.6.10, 5.5.11
#[gtest]
#[tokio::test]
async fn e006_04_01_delete_a_batch_of_existing_entities_with_the_same_id() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &BUILDING_SIMPLE_TERMS).await?;
    create_entity_with_context_string(&broker, &vocabulary, &building_simple_attributes(&namespace, 1)?).await?;
    let answer = broker
        .with_responses()
        .entities()
        .batch_delete(&[namespace.urn("Building", 1)?, namespace.urn("Building", 1)?])
        .send()
        .await;
    let result = partial_batch_outcome(answer)?;
    expect_partial_batch(&result, &[&namespace.urn("Building", 1)?], &namespace.urn("Building", 1)?)?;
    Ok(())
}
