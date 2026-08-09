//! Repository input parameter types for temporal data.

use crate::value_type::ValueType;
use andromeda_ngsi_ld_geometry::geometry::Geometry;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, attribute_type::AttributeType, instance_id::InstanceId, scope::grammar::ScopeBuf, uri::Uri};
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
    pub value_uri: Option<&'a Uri>,
    /// Additional metadata associated with the attribute instance.
    pub metadata: Option<&'a Value>,
}

/// Parameters for modifying an existing attribute instance in history.
#[derive(Debug)]
pub struct UpdateInstanceParams<'a> {
    /// Identifier of the attribute instance to update.
    pub instance_id: &'a InstanceId,
    /// Entity that owns the attribute.
    pub entity_id: &'a Uri,
    /// Expanded attribute name.
    pub attribute_name: &'a AttributeName,
    /// New typed value columns for this instance.
    pub values: HistoryValues<'a>,
    /// Updated observation timestamp, if provided.
    pub observed_at: Option<DateTime<Utc>>,
}

/// Input for a single TEA catalog entry in a bulk upsert.
#[derive(Debug)]
pub struct TeaBatchEntry {
    /// Entity identifier.
    pub entity_id: Uri,
    /// Expanded IRI of the attribute.
    pub attribute_name: AttributeName,
    /// NGSI-LD attribute meta-type.
    pub attribute_type: AttributeType,
    /// Multi-attribute discriminator; `None` for the default instance.
    pub dataset_id: Option<Uri>,
    /// Runtime value type discriminator.
    pub value_type: ValueType,
}

/// A single scope history observation used for both reads and bulk inserts.
#[derive(Debug)]
pub struct ScopeHistoryEntry {
    /// Observation timestamp.
    pub time: DateTime<Utc>,
    /// The entity this scope belongs to.
    pub entity_id: Uri,
    /// Scope path in ltree dot notation (slashes converted to dots).
    pub scope_path: ScopeBuf,
    /// Unique identifier for this scope observation instance.
    pub instance_id: InstanceId,
}
