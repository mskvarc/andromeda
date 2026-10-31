//! A batch upsert in which two of three entities share an id creates each id once, the later instance of the shared id
//! being applied after the first (clauses 5.5.11 and 5.6.8).

use crate::{
    batch_operations::expect_upsert_created,
    building_payloads::bare_building,
    entity_operations::{delete_entities, ld_json_body_client},
};
use andromeda_test_suite::{broker, namespace::Namespace, vocabulary::Vocabulary};
use googletest::prelude::*;

/// Pins: Check that one can upsert a batch of where two have the same id
/// Clause: 5.6.8, 5.5.11
#[gtest]
#[tokio::test]
async fn e004_07_01_upsert_a_batch_of_three_valid_entities_where_two_have_the_same_id() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Building"]).await?;
    let entities = [bare_building(&namespace, 1)?, bare_building(&namespace, 2)?, bare_building(&namespace, 1)?];
    let answer = ld_json_body_client(&broker, &vocabulary)?
        .with_responses()
        .entities()
        .batch_upsert(&entities)
        .send()
        .await;
    expect_upsert_created(answer, &[&namespace.urn("Building", 1)?, &namespace.urn("Building", 2)?])?;
    delete_entities(&broker, &[&namespace.urn("Building", 1)?, &namespace.urn("Building", 2)?]).await;
    Ok(())
}
