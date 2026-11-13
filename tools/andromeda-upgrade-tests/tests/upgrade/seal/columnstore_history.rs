//! Seals in history the columnstore holds are converted, and a top-level
//! seal's stream leaves it, however few rows the server lets one statement
//! decompress.

use crate::fixtures::{
    columnstore::{bound_decompression, convert_history_to_columnstore, history_chunks_in_columnstore},
    current_state::insert_entity,
    history::{InstanceIdentity, insert_history_row, insert_stream},
    observed::assert_typed_seal,
    rehearsal::{at_v1_0_0, intern_seal_attribute},
    statements::{read_column, read_documents},
    test_error::UpgradeTestError,
    vocabulary::{complete_seals, proof_node, seal_element},
};
use pretty_assertions::assert_eq;
use std::slice;

/// The instances of the sealed history rows. One converted batch holds all
/// three, more than the one row the bound lets a statement decompress.
const SEALED_INSTANCES: [&str; 3] = [
    "00000000-0000-4000-8000-0000000000f1",
    "00000000-0000-4000-8000-0000000000f2",
    "00000000-0000-4000-8000-0000000000f3",
];

/// The instance of the top-level seal's history row.
const TOP_LEVEL_INSTANCE: &str = "00000000-0000-4000-8000-0000000000f4";

#[tokio::test]
async fn seals_in_history_the_columnstore_holds_are_converted_past_the_decompression_bound() -> Result<(), UpgradeTestError> {
    let (mut database, keys) = at_v1_0_0().await?;
    let seal_key = intern_seal_attribute(database.pool()).await?;
    let entity_key = insert_entity(database.pool(), "urn:ngsi-ld:Store:001").await?;
    let temperature_stream = insert_stream(database.pool(), entity_key, keys.temperature).await?;
    let seal_stream = insert_stream(database.pool(), entity_key, seal_key).await?;
    let seal = seal_element(0, seal_key, &proof_node(), &complete_seals());
    for instance in SEALED_INSTANCES {
        insert_history_row(
            database.pool(),
            temperature_stream,
            &InstanceIdentity::Generated(instance),
            slice::from_ref(&seal),
        )
        .await?;
    }
    insert_history_row(database.pool(), seal_stream, &InstanceIdentity::Generated(TOP_LEVEL_INSTANCE), &[]).await?;
    convert_history_to_columnstore(database.pool()).await?;
    assert_eq!(history_chunks_in_columnstore(database.pool()).await?, [true]);
    bound_decompression(database.pool(), 1).await?;

    database.migrate_to_current().await?;

    assert_eq!(
        read_column::<i64>(database.pool(), "attribute_history", "stream_key").await?,
        [temperature_stream; 3]
    );
    let rows = read_documents(database.pool(), "attribute_history", "sub_attributes", "stream_key", None).await?;
    assert_eq!(rows.len(), SEALED_INSTANCES.len());
    for sub_attributes in rows {
        let sub_attributes = sub_attributes.ok_or(UpgradeTestError::Missing {
            what: "the history sub-attributes",
        })?;
        assert_typed_seal(&sub_attributes[0], seal_key, keys.store);
    }
    database.drop_database().await?;
    Ok(())
}
