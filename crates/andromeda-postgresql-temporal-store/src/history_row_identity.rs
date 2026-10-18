//! The identity and column reads the two history tables share.
//!
//! `attribute_history` and `scope_history` record a write identity and the
//! Temporal Property instants the same way, and an attribute instance also
//! carries an identifier split across two mutually exclusive columns; a Scope
//! instance has none (clause 4.5.6). Both decoders read those columns through
//! here, naming the table in any failure so the two are told apart.

use crate::{
    error::TemporalStoreError,
    stored_history_error::{HistoryColumn, HistoryTable, StoredHistoryError},
};
use andromeda_context_commit::{write_id::WriteId, write_ordinal::WriteOrdinal};
use andromeda_ngsi_ld_lexicon::{identifier::Identifier, instance_identity::InstanceIdentity};
use andromeda_temporal::append_intent::TemporalObservationIdentity;
use chrono::{DateTime, Utc};
use sqlx::{Decode, Postgres, Row, Type, postgres::PgRow};
use uuid::Uuid;

/// Reassemble the identifier an instance is published to clients under.
///
/// The two identifier columns are mutually exclusive by check constraint, so a
/// row populating both or neither is corrupt rather than ambiguous, and is
/// reported instead of being resolved by precedence.
pub(crate) fn decode_instance_identity(row: &PgRow, table: HistoryTable) -> Result<InstanceIdentity, TemporalStoreError> {
    let generated: Option<Uuid> = row.try_get("generated_instance_id")?;
    let verbatim: Option<String> = row.try_get("verbatim_instance_id")?;
    match (generated, verbatim) {
        (Some(uuid), None) => Ok(InstanceIdentity::Generated(uuid)),
        (None, Some(raw)) => Ok(InstanceIdentity::Verbatim(
            raw.parse::<Identifier>()
                .map_err(|source| StoredHistoryError::VerbatimInstanceId { table, source })?,
        )),
        (Some(_), Some(_)) => Err(StoredHistoryError::BothInstanceIdentifiers { table }.into()),
        (None, None) => Err(StoredHistoryError::NoInstanceIdentifier { table }.into()),
    }
}

/// Read the identity of the atomic write that produced the row.
pub(crate) fn decode_write_identity(row: &PgRow, table: HistoryTable) -> Result<TemporalObservationIdentity, TemporalStoreError> {
    let write_id: Uuid = row.try_get("write_id")?;
    let ordinal: i32 = row.try_get("write_ordinal")?;
    Ok(TemporalObservationIdentity {
        write_id: WriteId::from_uuid(write_id),
        ordinal: WriteOrdinal::try_from(ordinal).map_err(|source| StoredHistoryError::WriteOrdinal { table, source })?,
    })
}

/// Read a column the row's declared shape requires to hold a value.
pub(crate) fn required_column<'r, T>(row: &'r PgRow, table: HistoryTable, column: HistoryColumn) -> Result<T, TemporalStoreError>
where
    T: Decode<'r, Postgres> + Type<Postgres>,
{
    let stored: Option<T> = row.try_get(<&'static str>::from(column))?;
    Ok(stored.ok_or(StoredHistoryError::AbsentColumn { table, column })?)
}

/// Read a nullable timestamp column, preserving its absence.
pub(crate) fn nullable_instant(row: &PgRow, column: HistoryColumn) -> Result<Option<DateTime<Utc>>, TemporalStoreError> {
    Ok(row.try_get::<Option<DateTime<Utc>>, _>(<&'static str>::from(column))?)
}
