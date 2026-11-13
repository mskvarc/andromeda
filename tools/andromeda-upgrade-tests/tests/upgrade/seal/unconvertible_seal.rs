//! A seal the broker would not admit is dropped with its own sub-attributes
//! instead of being stored in a form it cannot read.

use crate::fixtures::{
    current_state::{insert_entity, insert_number_attribute, intern_attribute},
    observed::{current_sub_attributes, field_of_each},
    rehearsal::{at_v1_0_0, intern_seal_attribute},
    test_error::UpgradeTestError,
    vocabulary::{SEALED_TYPE, complete_seals, proof_node, proof_node_without, seal_element, seals, string_element},
};
use pretty_assertions::assert_eq;
use serde_json::json;

#[tokio::test]
async fn a_seal_naming_a_type_and_a_non_reference_is_dropped_with_its_sub_attributes() -> Result<(), UpgradeTestError> {
    let (mut database, keys) = at_v1_0_0().await?;
    let seal_key = intern_seal_attribute(database.pool()).await?;
    let note_key = intern_attribute(database.pool(), "https://example.org/note").await?;
    let entity_key = insert_entity(database.pool(), "urn:ngsi-ld:Store:001").await?;
    let mixed = seals(&json!([{"@id": SEALED_TYPE}, {"@value": 5}]));
    let elements = [seal_element(0, seal_key, &proof_node(), &mixed), string_element(1, note_key, "about the seal")];
    insert_number_attribute(database.pool(), entity_key, keys.temperature, 0, &elements).await?;

    database.migrate_to_current().await?;

    assert_eq!(current_sub_attributes(database.pool(), keys.temperature).await?, None);
    database.drop_database().await?;
    Ok(())
}

#[tokio::test]
async fn a_seal_missing_a_proof_member_is_dropped_and_the_sibling_behind_it_is_renumbered() -> Result<(), UpgradeTestError> {
    let (mut database, keys) = at_v1_0_0().await?;
    let seal_key = intern_seal_attribute(database.pool()).await?;
    let precision_key = intern_attribute(database.pool(), "https://example.org/precision").await?;
    let source_key = intern_attribute(database.pool(), "https://example.org/source").await?;
    let detail_key = intern_attribute(database.pool(), "https://example.org/detail").await?;
    let entity_key = insert_entity(database.pool(), "urn:ngsi-ld:Store:001").await?;
    let unsigned = proof_node_without("https://w3id.org/security#proofValue");
    let elements = [
        string_element(0, precision_key, "high"),
        seal_element(0, seal_key, &unsigned, &complete_seals()),
        string_element(2, detail_key, "below the seal"),
        string_element(0, source_key, "sensor"),
        string_element(4, detail_key, "below the sibling"),
    ];
    insert_number_attribute(database.pool(), entity_key, keys.temperature, 0, &elements).await?;

    database.migrate_to_current().await?;

    let kept = current_sub_attributes(database.pool(), keys.temperature)
        .await?
        .ok_or(UpgradeTestError::Missing { what: "the sub-attributes" })?;
    assert_eq!(field_of_each(&kept, "attribute_name_key"), json!([precision_key, source_key, detail_key]));
    assert_eq!(field_of_each(&kept, "parent"), json!([0, 0, 2]));
    assert_eq!(field_of_each(&kept, "value_string"), json!(["high", "sensor", "below the sibling"]));
    database.drop_database().await?;
    Ok(())
}
