//! A top-level `ngsildproof` signs no attribute: the upgrade removes it as a
//! delete of the instance would, from current state, search rows, census
//! ledgers and history.

use crate::fixtures::{
    census_ledger::{count_instance, count_stream, instances_by_attribute, instances_by_type_and_attribute, streams_by_type_and_attribute},
    current_state::{
        insert_entity,
        insert_entity_search,
        insert_evolving_entity,
        insert_number_attribute,
        insert_object_attribute,
        insert_type_member,
        intern_attribute,
        intern_type,
    },
    history::{InstanceIdentity, insert_claim, insert_history_row, insert_stream},
    rehearsal::{at_v1_0_0, intern_seal_attribute},
    statements::{count_rows, read_column},
    test_error::UpgradeTestError,
    vocabulary::{complete_seals, proof_node},
};
use pretty_assertions::assert_eq;
use std::collections::BTreeMap;

/// The client-supplied identifier of the seal's one recorded instance.
const SEAL_INSTANCE: &str = "urn:ngsi-ld:attribute:instance:seal-1";

#[tokio::test]
async fn a_top_level_seal_leaves_current_state_and_the_search_row() -> Result<(), UpgradeTestError> {
    let (mut database, keys) = at_v1_0_0().await?;
    let seal_key = intern_seal_attribute(database.pool()).await?;
    let humidity_key = intern_attribute(database.pool(), "https://example.org/humidity").await?;
    let entity_key = insert_entity(database.pool(), "urn:ngsi-ld:Store:001").await?;
    insert_entity_search(database.pool(), entity_key, &[keys.store], &[keys.temperature, seal_key, humidity_key]).await?;
    insert_number_attribute(database.pool(), entity_key, keys.temperature, 0, &[]).await?;
    insert_object_attribute(database.pool(), entity_key, seal_key, 1, &proof_node(), &complete_seals()).await?;
    insert_number_attribute(database.pool(), entity_key, humidity_key, 2, &[]).await?;

    database.migrate_to_current().await?;

    let held = read_column::<i32>(database.pool(), "current_attributes", "attribute_name_key").await?;
    assert_eq!(held, [keys.temperature, humidity_key]);
    assert_eq!(read_column::<i32>(database.pool(), "current_attributes", "ordinal").await?, [0, 1]);
    let searched = read_column::<Vec<i32>>(database.pool(), "entity_search", "attribute_name_keys").await?;
    assert_eq!(searched, [vec![keys.temperature, humidity_key]]);
    let instance_names = read_column::<Vec<i32>>(database.pool(), "entity_search", "attribute_instance_name_keys").await?;
    assert_eq!(instance_names, [vec![keys.temperature, humidity_key]]);
    let instance_kinds = read_column::<Vec<i16>>(database.pool(), "entity_search", "attribute_instance_kinds").await?;
    assert_eq!(instance_kinds, [vec![1_i16, 1]]);
    database.drop_database().await?;
    Ok(())
}

#[tokio::test]
async fn a_top_level_seal_nets_out_of_the_census_ledgers() -> Result<(), UpgradeTestError> {
    let (mut database, keys) = at_v1_0_0().await?;
    let seal_key = intern_seal_attribute(database.pool()).await?;
    let entity_key = insert_evolving_entity(database.pool(), "urn:ngsi-ld:Store:001", &[keys.store]).await?;
    insert_entity_search(database.pool(), entity_key, &[keys.store], &[keys.temperature, seal_key]).await?;
    insert_type_member(database.pool(), entity_key, keys.store, "urn:ngsi-ld:Store:001").await?;
    insert_number_attribute(database.pool(), entity_key, keys.temperature, 0, &[]).await?;
    insert_object_attribute(database.pool(), entity_key, seal_key, 1, &proof_node(), &complete_seals()).await?;
    count_instance(database.pool(), keys.store, keys.temperature).await?;
    count_instance(database.pool(), keys.store, seal_key).await?;
    let temperature_stream = insert_stream(database.pool(), entity_key, keys.temperature).await?;
    let seal_stream = insert_stream(database.pool(), entity_key, seal_key).await?;
    count_stream(database.pool(), keys.store, keys.temperature).await?;
    count_stream(database.pool(), keys.store, seal_key).await?;
    insert_history_row(
        database.pool(),
        temperature_stream,
        &InstanceIdentity::Generated("00000000-0000-4000-8000-0000000000f1"),
        &[],
    )
    .await?;
    insert_history_row(database.pool(), seal_stream, &InstanceIdentity::Verbatim(SEAL_INSTANCE), &[]).await?;
    insert_claim(database.pool(), entity_key, seal_key, SEAL_INSTANCE).await?;

    database.migrate_to_current().await?;

    assert_eq!(instances_by_attribute(database.pool()).await?, BTreeMap::from([(keys.temperature, 1)]));
    assert_eq!(
        instances_by_type_and_attribute(database.pool()).await?,
        BTreeMap::from([((keys.store, keys.temperature), 1)])
    );
    assert_eq!(
        streams_by_type_and_attribute(database.pool()).await?,
        BTreeMap::from([((keys.store, keys.temperature), 1)])
    );
    database.drop_database().await?;
    Ok(())
}

/// The broker counts a stream under the types its Entity's identity row
/// records for the Evolution, so the seed holds no type membership row.
#[tokio::test]
async fn a_top_level_seal_stream_is_counted_out_under_every_type_its_evolution_records() -> Result<(), UpgradeTestError> {
    let (mut database, keys) = at_v1_0_0().await?;
    let seal_key = intern_seal_attribute(database.pool()).await?;
    let shop_key = intern_type(database.pool(), "https://example.org/Shop").await?;
    let entity_key = insert_evolving_entity(database.pool(), "urn:ngsi-ld:Store:001", &[keys.store, shop_key]).await?;
    insert_stream(database.pool(), entity_key, seal_key).await?;
    count_stream(database.pool(), keys.store, seal_key).await?;
    count_stream(database.pool(), shop_key, seal_key).await?;

    database.migrate_to_current().await?;

    assert_eq!(streams_by_type_and_attribute(database.pool()).await?, BTreeMap::new());
    database.drop_database().await?;
    Ok(())
}

#[tokio::test]
async fn a_top_level_seal_takes_its_history_streams_rows_and_claims_with_it() -> Result<(), UpgradeTestError> {
    let (mut database, keys) = at_v1_0_0().await?;
    let seal_key = intern_seal_attribute(database.pool()).await?;
    let entity_key = insert_entity(database.pool(), "urn:ngsi-ld:Store:001").await?;
    insert_object_attribute(database.pool(), entity_key, seal_key, 0, &proof_node(), &complete_seals()).await?;
    let temperature_stream = insert_stream(database.pool(), entity_key, keys.temperature).await?;
    let seal_stream = insert_stream(database.pool(), entity_key, seal_key).await?;
    insert_history_row(
        database.pool(),
        temperature_stream,
        &InstanceIdentity::Generated("00000000-0000-4000-8000-0000000000f1"),
        &[],
    )
    .await?;
    insert_history_row(database.pool(), seal_stream, &InstanceIdentity::Verbatim(SEAL_INSTANCE), &[]).await?;
    insert_claim(database.pool(), entity_key, seal_key, SEAL_INSTANCE).await?;

    database.migrate_to_current().await?;

    assert_eq!(
        read_column::<i64>(database.pool(), "temporal_attribute_streams", "stream_key").await?,
        [temperature_stream]
    );
    assert_eq!(
        read_column::<i64>(database.pool(), "attribute_history", "stream_key").await?,
        [temperature_stream]
    );
    assert_eq!(count_rows(database.pool(), "attribute_history_instance_claims").await?, 0);
    database.drop_database().await?;
    Ok(())
}
