//! A complete seal held in current state becomes a typed seal.

use crate::{
    current_state::{insert_entity, insert_number_attribute},
    observed::{assert_typed_seal, current_proofs_as_written, current_sub_attributes},
    rehearsal::{at_v1_0_0, intern_seal_attribute},
    test_error::UpgradeTestError,
    vocabulary::{complete_seals, proof_node, seal_element},
};
use pretty_assertions::assert_eq;

#[tokio::test]
async fn a_complete_seal_becomes_a_typed_seal_naming_the_interned_type() -> Result<(), UpgradeTestError> {
    let (mut database, keys) = at_v1_0_0().await?;
    let seal_key = intern_seal_attribute(database.pool()).await?;
    let entity_key = insert_entity(database.pool(), "urn:ngsi-ld:Store:001").await?;
    let seal = seal_element(0, seal_key, &proof_node(), &complete_seals());
    insert_number_attribute(database.pool(), entity_key, keys.temperature, 0, &[seal]).await?;

    database.migrate_to_current().await?;

    let sub_attributes = current_sub_attributes(database.pool(), keys.temperature)
        .await?
        .ok_or(UpgradeTestError::Missing { what: "the sub-attributes" })?;
    assert_eq!(sub_attributes.as_array().map(Vec::len), Some(1));
    assert_typed_seal(&sub_attributes[0], seal_key, keys.store);
    assert_eq!(current_proofs_as_written(database.pool()).await?, [proof_node()]);
    database.drop_database().await?;
    Ok(())
}
