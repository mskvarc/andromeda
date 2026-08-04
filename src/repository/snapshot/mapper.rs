//! Row-to-domain conversions for snapshot records.

use crate::{
    core::error::AndromedaError,
    domain::{
        ngsi_ld_urn::SnapshotId,
        snapshot::{ExecutionResultDetails, KeyValuePair, Snapshot, SnapshotPriority, SnapshotStatus},
    },
};
use iso8601_duration::Duration as Iso8601Duration;
use serde_json::Value;
use sqlx::Row;
use url::Url;

/// Deserialize an optional JSON column into a typed `Option<T>`.
pub fn deserialize_json_column<T: serde::de::DeserializeOwned>(row: &sqlx::postgres::PgRow, column: &str) -> Result<Option<T>, AndromedaError> {
    let val: Option<Value> = row.try_get(column)?;
    val.map(|v| serde_json::from_value(v).map_err(|e| AndromedaError::internal(format!("Failed to deserialize {column}: {e}"))))
        .transpose()
}

/// Convert a `snapshots` row into the domain [`Snapshot`].
pub fn row_to_snapshot(row: &sqlx::postgres::PgRow) -> Result<Snapshot, AndromedaError> {
    let id_str: String = row.try_get("id")?;
    let id = id_str
        .parse::<SnapshotId>()
        .map_err(|e| AndromedaError::internal(format!("Invalid snapshot id '{id_str}': {e}")))?;

    // `status` is projected as `status::text`; the label mapping lives in the
    // domain's `EnumString`, so an unexpected label is a typed parse error
    // rather than an opaque sqlx decode failure.
    let status_label: String = row.try_get("status")?;
    let status = status_label
        .parse::<SnapshotStatus>()
        .map_err(|e| AndromedaError::internal(format!("Invalid snapshot status '{status_label}': {e}")))?;

    let snapshot_lifetime = row
        .try_get::<Option<String>, _>("snapshot_lifetime")?
        .as_deref()
        .map(str::parse::<Iso8601Duration>)
        .transpose()
        .map_err(|e| AndromedaError::internal(format!("Invalid snapshot lifetime: {e:?}")))?;
    let snapshot_priority_raw: i16 = row.try_get("snapshot_priority")?;
    let snapshot_priority = SnapshotPriority::try_from(snapshot_priority_raw)
        .map_err(|e| AndromedaError::internal(format!("Invalid snapshot priority '{snapshot_priority_raw}': {e}")))?;
    let endpoint = row
        .try_get::<Option<String>, _>("endpoint")?
        .map(|value| Url::parse(&value))
        .transpose()
        .map_err(|e| AndromedaError::internal(format!("Invalid endpoint URI: {e}")))?;

    let queries_details: Option<Vec<ExecutionResultDetails>> = deserialize_json_column(row, "queries_details")?;
    let temporal_queries_details: Option<Vec<ExecutionResultDetails>> = deserialize_json_column(row, "temporal_queries_details")?;
    let snapshot_queries: Option<Vec<Value>> = deserialize_json_column(row, "snapshot_queries")?;
    let snapshot_temporal_queries: Option<Vec<Value>> = deserialize_json_column(row, "snapshot_temporal_queries")?;
    let receiver_info: Option<Vec<KeyValuePair>> = deserialize_json_column(row, "receiver_info")?;

    Ok(Snapshot {
        id,
        snapshot_queries,
        snapshot_temporal_queries,
        snapshot_lifetime,
        snapshot_priority,
        endpoint,
        receiver_info,
        status,
        queries_details,
        temporal_queries_details,
        created_at: row.try_get("created_at")?,
        modified_at: row.try_get("modified_at")?,
        expires_at: row.try_get("expires_at")?,
        last_used_at: row.try_get("last_used_at")?,
    })
}
