//! A batch of three entities in which two share an id creates the two distinct entities and reports the repeated id in
//! error (clauses 5.6.7 and 5.5.11).

use crate::{
    batch_operations::{batch_create_partial, expect_partial_batch},
    building_payloads::bare_building,
    entity_operations::{delete_entities, ld_json_body_client},
};
use andromeda_test_suite::{broker, namespace::Namespace, vocabulary::Vocabulary};
use googletest::prelude::*;

/// Pins: Check that one can create a batch of entities where two have the same id
/// Clause: 5.6.7, 5.5.11
#[gtest]
#[tokio::test]
async fn e003_10_01_create_a_batch_of_three_valid_entities_where_two_have_the_same_id() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Building"]).await?;
    let entities = [bare_building(&namespace, 1)?, bare_building(&namespace, 2)?, bare_building(&namespace, 1)?];
    let (first, second) = (namespace.urn("Building", 1)?, namespace.urn("Building", 2)?);
    let result = batch_create_partial(&ld_json_body_client(&broker, &vocabulary)?, &entities).await?;
    expect_partial_batch(&result, &[&first, &second], &first)?;
    delete_entities(&broker, &[&first, &second]).await;
    Ok(())
}
