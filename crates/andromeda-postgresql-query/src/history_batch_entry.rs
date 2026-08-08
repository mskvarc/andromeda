//! Input for a single attribute history observation in a bulk insert.
//!
//! `HistoryBatchEntry` is defined here, in the query crate, because both the
//! context store and the temporal store need it: the temporal store records
//! the observation, and the context store's `attribute_current` projection
//! reads the same batch to materialise the latest value per TEA. Neither
//! store may depend on the other, so the struct lives below both.

use andromeda_ngsi_ld_geometry::geometry::Geometry;
use chrono::{DateTime, Utc};
use serde_json::Value;

/// Input for a single attribute history observation in a bulk insert.
#[derive(Debug, Clone)]
pub struct HistoryBatchEntry {
    /// TimescaleDB partition key.
    pub time: DateTime<Utc>,
    /// Foreign key into the TEA catalog.
    pub tea_id: uuid::Uuid,
    /// Numeric value column.
    pub value_number: Option<f64>,
    /// String value column.
    pub value_string: Option<String>,
    /// Boolean value column.
    pub value_bool: Option<bool>,
    /// Geometry value column.
    pub value_geo: Option<Geometry>,
    /// JSON object value column.
    pub value_object: Option<Value>,
    /// DateTime value column.
    pub value_datetime: Option<DateTime<Utc>>,
    /// URI value column (for Relationships).
    pub value_uri: Option<String>,
    /// Unique instance identifier.
    pub instance_id: String,
    /// Application-level observation timestamp.
    pub observed_at: Option<DateTime<Utc>>,
    /// Additional metadata (sub-attributes, unitCode, etc.).
    pub metadata: Option<Value>,
    /// Server wall-clock instant at which this observation was produced.
    ///
    /// Distinct from `time`, which carries the client-supplied `observedAt`
    /// when present and is therefore not a write-order key. `recorded_at`
    /// gives the `attribute_current` projection last-write-wins semantics so
    /// it tracks `entities.payload` rather than `observedAt` order.
    pub recorded_at: DateTime<Utc>,
    /// True when `instance_id` came from the client's `instanceId` JSON-LD
    /// term (NGSI-LD section 4.5.7) rather than being server-generated. Drives
    /// whether the broker must run the dedup DELETE before INSERT.
    pub instance_id_client_supplied: bool,
}
