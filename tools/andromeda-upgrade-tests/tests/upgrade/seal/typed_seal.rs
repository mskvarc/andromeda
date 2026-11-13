//! A complete seal held in current state becomes a typed seal the broker
//! reads, naming its types by their dictionary keys.

use crate::fixtures::{
    current_state::{insert_entity, insert_number_attribute},
    observed::{assert_typed_seal, current_proofs_as_written, current_seals_as_the_broker_reads, current_sub_attributes},
    rehearsal::{at_v1_0_0, intern_seal_attribute},
    statements::{count_rows, read_column},
    test_error::UpgradeTestError,
    vocabulary::{PROOF_CREATED, SEALED_ENTITY_ID, SEALED_TYPE, complete_seals, proof_node, seal_element, seals},
};
use pretty_assertions::assert_eq;
use serde_json::json;

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
    let read = current_seals_as_the_broker_reads(database.pool()).await?;
    assert_eq!(read.len(), 1);
    assert_eq!(read[0].entity_id.as_str(), SEALED_ENTITY_ID);
    let sealed: Vec<i32> = read[0].entity_type_keys.iter().map(|key| key.get()).collect();
    assert_eq!(sealed, [keys.store]);
    assert_eq!(read[0].created.to_string(), PROOF_CREATED);
    assert_eq!(read[0].verification_method.as_str(), "https://example.org/keys/1#key-1");
    assert_eq!(read[0].proof_purpose.as_str(), "https://w3id.org/security#assertionMethod");
    assert_eq!(read[0].cryptosuite.as_str(), "eddsa-rdfc-2022");
    assert_eq!(read[0].proof_value.as_str(), "z58DAdFfa9SkqZMVPxAQp");
    assert!(read[0].members.is_some());
    database.drop_database().await?;
    Ok(())
}

/// The type dictionary holds a type no Entity carries once a seal names it,
/// and a type named twice is sealed once, at its first position.
#[tokio::test]
async fn a_seal_naming_a_type_no_entity_carries_interns_it_and_names_each_type_once() -> Result<(), UpgradeTestError> {
    const UNSEEN_TYPE: &str = "https://example.org/Warehouse";
    let (mut database, keys) = at_v1_0_0().await?;
    let seal_key = intern_seal_attribute(database.pool()).await?;
    let entity_key = insert_entity(database.pool(), "urn:ngsi-ld:Store:001").await?;
    let subproperties = seals(&json!([{"@id": UNSEEN_TYPE}, {"@id": SEALED_TYPE}, {"@id": UNSEEN_TYPE}]));
    let seal = seal_element(0, seal_key, &proof_node(), &subproperties);
    insert_number_attribute(database.pool(), entity_key, keys.temperature, 0, &[seal]).await?;

    database.migrate_to_current().await?;

    assert_eq!(count_rows(database.pool(), "entity_type_names").await?, 2);
    let names: Vec<String> = read_column(database.pool(), "entity_type_names", "name").await?;
    assert_eq!(names, [SEALED_TYPE, UNSEEN_TYPE]);
    let read = current_seals_as_the_broker_reads(database.pool()).await?;
    assert_eq!(read.len(), 1);
    let sealed: Vec<i32> = read[0].entity_type_keys.iter().map(|key| key.get()).collect();
    assert_eq!(sealed.len(), 2);
    assert_ne!(sealed[0], keys.store);
    assert_eq!(sealed[1], keys.store);
    database.drop_database().await?;
    Ok(())
}
