//! Row-to-domain conversions for snapshot records.

use crate::{
    error::ContextStoreError,
    jsonb_text::JsonbText,
    snapshot::{status_code::status_of, stored_snapshot_error::StoredSnapshotError},
};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::{
    key_value_pair::KeyValuePair,
    query::body::Query,
    snapshot::{ExecutionResultDetails, Snapshot, SnapshotPriority},
};
use andromeda_postgresql_codes::snapshot_status::SnapshotStatusCode;
use andromeda_postgresql_schema::snapshot::{SnapshotPreparation, Snapshots};
use iso8601_duration::Duration as Iso8601Duration;
use sea_query::Iden;
use serde::de::DeserializeOwned;
use sqlx::{Row, postgres::PgRow};
use url::Url;

/// Deserialize an optional JSON column into a typed `Option<T>`, decoded by
/// the streaming reader straight from the column's text.
///
/// `unreadable` names the column in the failure, so the JSON columns of one
/// record are told apart without carrying the column's text.
///
/// # Errors
///
/// Returns [`ContextStoreError::Database`] when the column cannot be read, and
/// [`ContextStoreError::StoredSnapshot`] when the stored JSON does not
/// deserialize into `T`.
pub fn deserialize_json_column<T: DeserializeOwned>(
    row: &PgRow,
    column: impl Iden + Copy,
    unreadable: fn(serde_json::Error) -> StoredSnapshotError,
) -> Result<Option<T>, ContextStoreError> {
    let text: Option<JsonbText<'_>> = row.try_get(column.unquoted())?;
    Ok(text.map(|text| serde_json::from_slice(text.as_bytes()).map_err(unreadable)).transpose()?)
}

/// Convert a `snapshots` row into the domain [`Snapshot`].
///
/// # Errors
///
/// Returns [`ContextStoreError::Database`] when a column cannot be read, and
/// [`ContextStoreError::StoredSnapshot`] when a stored value no longer parses
/// back into its domain type: the Snapshot URI, the status code, the lifetime
/// duration, the priority, the endpoint URL, or one of the JSON-encoded members.
pub fn row_to_snapshot(row: &PgRow) -> Result<Snapshot, ContextStoreError> {
    let id_str: String = row.try_get("id")?;
    let id = id_str.parse::<Identifier>().map_err(StoredSnapshotError::Id)?;

    // An unassigned status code is a typed decode failure naming the registry
    // rather than an opaque sqlx one.
    let status_code: i16 = row.try_get("status")?;
    let status = status_of(SnapshotStatusCode::try_from(status_code).map_err(StoredSnapshotError::Status)?);

    let snapshot_lifetime = row
        .try_get::<Option<String>, _>("snapshot_lifetime")?
        .as_deref()
        .map(str::parse::<Iso8601Duration>)
        .transpose()
        .map_err(|error| StoredSnapshotError::Lifetime { position: error.position })?;
    // The `snapshot_priority` column is `SMALLINT`; narrow it back to the byte
    // the domain type validates before the domain range check runs.
    let snapshot_priority_raw: i16 = row.try_get("snapshot_priority")?;
    let snapshot_priority_byte = u8::try_from(snapshot_priority_raw).map_err(StoredSnapshotError::PriorityWidth)?;
    let snapshot_priority = SnapshotPriority::try_from(snapshot_priority_byte).map_err(StoredSnapshotError::Priority)?;
    let endpoint = row
        .try_get::<Option<String>, _>("endpoint")?
        .map(|value| Url::parse(&value))
        .transpose()
        .map_err(StoredSnapshotError::Endpoint)?;

    let queries_details: Option<Vec<ExecutionResultDetails>> =
        deserialize_json_column(row, SnapshotPreparation::QueriesDetails, StoredSnapshotError::QueriesDetails)?;
    let temporal_queries_details: Option<Vec<ExecutionResultDetails>> =
        deserialize_json_column(row, SnapshotPreparation::TemporalQueriesDetails, StoredSnapshotError::TemporalQueriesDetails)?;
    let snapshot_queries: Option<Vec<Query>> = deserialize_json_column(row, Snapshots::SnapshotQueries, StoredSnapshotError::SnapshotQueries)?;
    let snapshot_temporal_queries: Option<Vec<Query>> =
        deserialize_json_column(row, Snapshots::SnapshotTemporalQueries, StoredSnapshotError::SnapshotTemporalQueries)?;
    let receiver_info: Option<Vec<KeyValuePair>> = deserialize_json_column(row, Snapshots::ReceiverInfo, StoredSnapshotError::ReceiverInfo)?;

    Ok(Snapshot::builder()
        .id(id)
        .snapshot_queries(snapshot_queries)
        .snapshot_temporal_queries(snapshot_temporal_queries)
        .snapshot_lifetime(snapshot_lifetime)
        .snapshot_priority(snapshot_priority)
        .endpoint(endpoint)
        .receiver_info(receiver_info)
        .status(status)
        .queries_details(queries_details)
        .temporal_queries_details(temporal_queries_details)
        .created_at(row.try_get("created_at")?)
        .modified_at(row.try_get("modified_at")?)
        .expires_at(row.try_get("expires_at")?)
        .last_used_at(row.try_get("last_used_at")?)
        .build())
}
