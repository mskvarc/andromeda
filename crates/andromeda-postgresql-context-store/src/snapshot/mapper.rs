//! Row-to-domain conversions for snapshot records.

use crate::error::ContextStoreError;
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_model::{
    key_value_pair::KeyValuePair,
    query::body::Query,
    snapshot::{ExecutionResultDetails, Snapshot, SnapshotPriority, SnapshotStatus},
};
use iso8601_duration::Duration as Iso8601Duration;
use serde::de::DeserializeOwned;
use serde_json::Value;
use sqlx::{Row, postgres::PgRow};
use url::Url;

/// Deserialize an optional JSON column into a typed `Option<T>`.
pub fn deserialize_json_column<T: DeserializeOwned>(row: &PgRow, column: &str) -> Result<Option<T>, ContextStoreError> {
    let val: Option<Value> = row.try_get(column)?;
    val.map(|v| serde_json::from_value(v).map_err(|e| ContextStoreError::Internal(format!("Failed to deserialize {column}: {e}"))))
        .transpose()
}

/// Convert a `snapshots` row into the domain [`Snapshot`].
pub fn row_to_snapshot(row: &PgRow) -> Result<Snapshot, ContextStoreError> {
    let id_str: String = row.try_get("id")?;
    let id = id_str
        .parse::<Uri>()
        .map_err(|e| ContextStoreError::Internal(format!("Invalid snapshot id '{id_str}': {e}")))?;

    // `status` is projected as `status::text`; the label mapping lives in the
    // domain's `EnumString`, so an unexpected label is a typed parse error
    // rather than an opaque sqlx decode failure.
    let status_label: String = row.try_get("status")?;
    let status = status_label
        .parse::<SnapshotStatus>()
        .map_err(|e| ContextStoreError::Internal(format!("Invalid snapshot status '{status_label}': {e}")))?;

    let snapshot_lifetime = row
        .try_get::<Option<String>, _>("snapshot_lifetime")?
        .as_deref()
        .map(str::parse::<Iso8601Duration>)
        .transpose()
        .map_err(|e| ContextStoreError::Internal(format!("Invalid snapshot lifetime: {e:?}")))?;
    // The `snapshot_priority` column is `SMALLINT`; narrow it back to the byte
    // the domain type validates. A value outside `u8` cannot be a valid
    // priority, so it maps to `0`, which the range check then rejects.
    let snapshot_priority_raw: i16 = row.try_get("snapshot_priority")?;
    let snapshot_priority = SnapshotPriority::try_from(u8::try_from(snapshot_priority_raw).unwrap_or(0))
        .map_err(|e| ContextStoreError::Internal(format!("Invalid snapshot priority '{snapshot_priority_raw}': {e}")))?;
    let endpoint = row
        .try_get::<Option<String>, _>("endpoint")?
        .map(|value| Url::parse(&value))
        .transpose()
        .map_err(|e| ContextStoreError::Internal(format!("Invalid endpoint URI: {e}")))?;

    let queries_details: Option<Vec<ExecutionResultDetails>> = deserialize_json_column(row, "queries_details")?;
    let temporal_queries_details: Option<Vec<ExecutionResultDetails>> = deserialize_json_column(row, "temporal_queries_details")?;
    let snapshot_queries: Option<Vec<Query>> = deserialize_json_column(row, "snapshot_queries")?;
    let snapshot_temporal_queries: Option<Vec<Query>> = deserialize_json_column(row, "snapshot_temporal_queries")?;
    let receiver_info: Option<Vec<KeyValuePair>> = deserialize_json_column(row, "receiver_info")?;

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
