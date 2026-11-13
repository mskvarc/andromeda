//! A branch holding seals as v1.0.0 stored them: an Entity whose attribute
//! carries a seal, beside a top-level `ngsildproof` the upgrade removes, both
//! in the search row and counted in the discovery census. A branch whose
//! sub-attributes are held aside cannot take the seal conversion.

use crate::fixtures::{
    census_ledger::{count_instance, instances_by_attribute},
    current_state::{insert_entity, insert_entity_search, insert_number_attribute, insert_object_attribute},
    observed::{assert_typed_seal, current_sub_attributes, field_of_each},
    rehearsal::Keys,
    statements::{name, read_column, read_documents},
    test_error::UpgradeTestError,
    vocabulary::{complete_seals, proof_node, seal_element},
};
use andromeda_context_space::{snapshot_branch::SnapshotBranch, snapshot_key::SnapshotKey};
use andromeda_postgresql_branch::snapshot_schema;
use andromeda_postgresql_context_store::migration_set::migration_version::MigrationVersion;
use andromeda_postgresql_seaquery::command::Command;
use pretty_assertions::assert_eq;
use sea_query::Table;
use serde_json::json;
use sqlx::PgPool;
use std::collections::BTreeMap;

/// The branch migration that converts the seals of the branch it runs in.
pub(crate) const SEAL_CONVERSION: MigrationVersion = MigrationVersion::new(33);

/// The Entity a sealed branch holds.
const ENTITY: &str = "urn:ngsi-ld:Store:001";

/// The column the sub-attributes of a branch are held aside in, which the
/// seal conversion does not name.
const HELD_ASIDE: &str = "sub_attributes_held_aside";

/// The column a branch keeps the sub-attributes of its attribute instances in.
const SUB_ATTRIBUTES: &str = "sub_attributes";

/// Writes the sealed Entity into the branch `pool` resolves to.
pub(crate) async fn seed_sealed_entity(pool: &PgPool, keys: Keys, seal_key: i32) -> Result<(), UpgradeTestError> {
    let entity_key = insert_entity(pool, ENTITY).await?;
    insert_entity_search(pool, entity_key, &[keys.store], &[keys.temperature, seal_key]).await?;
    let seal = seal_element(0, seal_key, &proof_node(), &complete_seals());
    insert_number_attribute(pool, entity_key, keys.temperature, 0, &[seal]).await?;
    insert_object_attribute(pool, entity_key, seal_key, 1, &proof_node(), &complete_seals()).await?;
    count_instance(pool, keys.store, keys.temperature).await?;
    count_instance(pool, keys.store, seal_key).await
}

/// Holds the branch `pool` resolves to to its converted seals: the
/// attribute's seal typed, the top-level seal gone from the current state and
/// the search row, and the census counting the attribute alone.
pub(crate) async fn assert_converted(pool: &PgPool, keys: Keys, seal_key: i32) -> Result<(), UpgradeTestError> {
    let sub_attributes = current_sub_attributes(pool, keys.temperature)
        .await?
        .ok_or(UpgradeTestError::Missing { what: "the sub-attributes" })?;
    assert_eq!(sub_attributes.as_array().map(Vec::len), Some(1));
    assert_typed_seal(&sub_attributes[0], seal_key, keys.store);
    assert_eq!(read_column::<i32>(pool, "current_attributes", "attribute_name_key").await?, [keys.temperature]);
    assert_eq!(
        read_column::<Vec<i32>>(pool, "entity_search", "attribute_name_keys").await?,
        [vec![keys.temperature]]
    );
    assert_eq!(instances_by_attribute(pool).await?, BTreeMap::from([(keys.temperature, 1)]));
    Ok(())
}

/// Moves the sub-attributes of the branch schema `branch` of the Snapshot
/// `key` aside, so the seal conversion fails in that branch alone.
pub(crate) async fn hold_sub_attributes_aside(pool: &PgPool, key: SnapshotKey, branch: SnapshotBranch) -> Result<(), UpgradeTestError> {
    rename_sub_attributes(pool, key, branch, SUB_ATTRIBUTES, HELD_ASIDE).await
}

/// Puts the sub-attributes [`hold_sub_attributes_aside`] moved back.
pub(crate) async fn restore_sub_attributes(pool: &PgPool, key: SnapshotKey, branch: SnapshotBranch) -> Result<(), UpgradeTestError> {
    rename_sub_attributes(pool, key, branch, HELD_ASIDE, SUB_ATTRIBUTES).await
}

/// Renames the sub-attributes column of the current state of one branch.
async fn rename_sub_attributes(pool: &PgPool, key: SnapshotKey, branch: SnapshotBranch, from: &str, to: &str) -> Result<(), UpgradeTestError> {
    Table::alter()
        .table((snapshot_schema(key, branch), name("current_attributes")))
        .rename_column(name(from), name(to))
        .to_owned()
        .to_query()
        .execute(pool)
        .await?;
    Ok(())
}

/// Holds the branch `pool` resolves to, whose sub-attributes are held aside,
/// to the seal of its attribute as v1.0.0 stored it.
pub(crate) async fn assert_held_seal_unconverted(pool: &PgPool) -> Result<(), UpgradeTestError> {
    let held = read_documents(pool, "current_attributes", HELD_ASIDE, "attribute_name_key", None).await?;
    let elements = held
        .iter()
        .flatten()
        .find(|elements| elements.as_array().is_some_and(|elements| !elements.is_empty()))
        .ok_or(UpgradeTestError::Missing { what: "the held seal" })?;
    assert_eq!(field_of_each(elements, "value_kind"), json!([5]));
    assert_eq!(field_of_each(elements, "proof_seals"), json!([complete_seals()]));
    Ok(())
}
