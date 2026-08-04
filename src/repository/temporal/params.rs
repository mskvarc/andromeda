//! Repository input parameter types for temporal data.

use crate::domain::{attribute_type::AttributeType, geometry::Geometry, temporal::ValueType};
use chrono::{DateTime, Utc};
use serde_json::Value;

/// Whether to delete all instances of an attribute or a single specific instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeleteScope {
    /// Delete all instances of the attribute regardless of dataset ID.
    All,
    /// Delete only the instance matching the specific dataset ID (or the default if `None`).
    Single,
}

/// Typed value columns for an attribute history row (exactly one should be set).
#[derive(Debug)]
pub struct HistoryValues<'a> {
    /// Numeric attribute value.
    pub value_number: Option<f64>,
    /// String attribute value.
    pub value_string: Option<&'a str>,
    /// Boolean attribute value.
    pub value_bool: Option<bool>,
    /// Geometry attribute value.
    pub value_geo: Option<&'a Geometry>,
    /// JSON object attribute value.
    pub value_object: Option<&'a Value>,
    /// DateTime attribute value.
    pub value_datetime: Option<DateTime<Utc>>,
    /// URI attribute value.
    pub value_uri: Option<&'a str>,
    /// Additional metadata associated with the attribute instance.
    pub metadata: Option<&'a Value>,
}

/// Parameters for modifying an existing attribute instance in history.
#[derive(Debug)]
pub struct UpdateInstanceParams<'a> {
    /// Identifier of the attribute instance to update.
    pub instance_id: &'a str,
    /// Entity that owns the attribute.
    pub entity_id: &'a str,
    /// Expanded attribute name.
    pub attribute_name: &'a str,
    /// New typed value columns for this instance.
    pub values: HistoryValues<'a>,
    /// Updated observation timestamp, if provided.
    pub observed_at: Option<DateTime<Utc>>,
}

/// Input for a single TEA catalog entry in a bulk upsert.
#[derive(Debug)]
pub struct TeaBatchEntry {
    /// Entity identifier.
    pub entity_id: String,
    /// Expanded IRI of the attribute.
    pub attribute_name: String,
    /// NGSI-LD attribute meta-type.
    pub attribute_type: AttributeType,
    /// Multi-attribute discriminator; `None` for the default instance.
    pub dataset_id: Option<String>,
    /// Runtime value type discriminator.
    pub value_type: ValueType,
}

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

/// A single scope history observation used for both reads and bulk inserts.
#[derive(Debug)]
pub struct ScopeHistoryEntry {
    /// Observation timestamp.
    pub time: DateTime<Utc>,
    /// The entity this scope belongs to.
    pub entity_id: String,
    /// Scope path in ltree dot notation (slashes converted to dots).
    pub scope_path: String,
    /// Unique identifier for this scope observation instance.
    pub instance_id: String,
}
