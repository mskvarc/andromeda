//! History in the columnstore, as the broker keeps it. No migration turns the
//! columnstore on: the broker does at runtime, with the layout the temporal
//! store declares, and converts every chunk whose range has ended.

use crate::fixtures::test_error::UpgradeTestError;
use andromeda_postgresql_seaquery::{
    columnstore_conversion::{ChunkReference, ConvertToColumnstore, RowstoreRemoval},
    columnstore_enablement::EnableColumnstore,
    command::Command,
    setting::{Setting, SettingAssignment, SettingScope},
    statement::Statement,
    timescale::{ChunkIsCompressed, ChunkRelationColumn, chunk_states_statement},
};
use andromeda_postgresql_temporal_store::hypertable_policy::HypertableName;
use sea_query::Iden;
use sqlx::{PgPool, Row};

/// Turns the columnstore of history on with the layout the broker gives it,
/// and converts every chunk of history to it.
pub(crate) async fn convert_history_to_columnstore(pool: &PgPool) -> Result<(), UpgradeTestError> {
    let history = HypertableName::AttributeHistory;
    EnableColumnstore::new(history.table(), history.columnstore_layout())
        .to_query()
        .execute(pool)
        .await?;
    for row in chunk_states_statement(history.into()).to_query().fetch_all(pool).await? {
        let chunk: String = row.try_get(ChunkRelationColumn.unquoted())?;
        ConvertToColumnstore::new(ChunkReference::Name(chunk), RowstoreRemoval::TruncateWhenFree)
            .to_query()
            .execute(pool)
            .await?;
    }
    Ok(())
}

/// Whether each chunk of history is in the columnstore, in range order.
pub(crate) async fn history_chunks_in_columnstore(pool: &PgPool) -> Result<Vec<bool>, UpgradeTestError> {
    let rows = chunk_states_statement(HypertableName::AttributeHistory.into())
        .to_query()
        .fetch_all(pool)
        .await?;
    Ok(rows.iter().map(|row| row.try_get(ChunkIsCompressed.unquoted())).collect::<Result<_, _>>()?)
}

/// Lets one statement of the session decompress at most `rows` rows, as an
/// operator may bound it on the server. The session is the one connection of
/// the rehearsal's pool, which the migrations run on.
pub(crate) async fn bound_decompression(pool: &PgPool, rows: u32) -> Result<(), UpgradeTestError> {
    SettingAssignment::new(
        Setting::TimescaledbMaxTuplesDecompressedPerDmlTransaction,
        rows.to_string(),
        SettingScope::Session,
    )
    .to_query()
    .execute(pool)
    .await?;
    Ok(())
}
