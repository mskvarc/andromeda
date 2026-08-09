//! Backend-neutral temporal observations and aggregation values.

use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, attribute_type::AttributeType, instance_id::InstanceId, uri::Uri};
use chrono::{DateTime, Utc};
use serde_json::Value;

/// A single decoded value from temporal history storage.
#[derive(Debug, Clone, PartialEq)]
pub enum TemporalValue {
    /// A numeric attribute value.
    Number(f64),
    /// A string attribute value.
    String(String),
    /// A boolean attribute value.
    Boolean(bool),
    /// A GeoJSON geometry in canonical JSON form.
    Geometry(Value),
    /// A JSON object attribute value.
    Object(Value),
    /// A datetime attribute value.
    DateTime(DateTime<Utc>),
    /// A URI attribute value.
    Uri(Uri),
    /// A deletion marker observation.
    Tombstone,
}

/// One decoded temporal attribute observation.
#[derive(Debug, Clone, PartialEq)]
pub struct TemporalHistoryRow {
    /// Expanded attribute IRI.
    pub attribute_name: AttributeName,
    /// NGSI-LD attribute family.
    pub attribute_type: AttributeType,
    /// Multi-attribute discriminator, absent for the default instance.
    pub dataset_id: Option<Uri>,
    /// Decoded observation value.
    pub value: TemporalValue,
    /// Client-declared observation timestamp.
    pub observed_at: Option<DateTime<Utc>>,
    /// Store-recorded temporal position.
    pub time: DateTime<Utc>,
    /// Stable temporal attribute instance identifier.
    pub instance_id: InstanceId,
    /// Additional expanded metadata attached to the instance.
    pub metadata: Option<Value>,
    /// Total observations in the attribute partition when requested by the query.
    pub partition_total: Option<i64>,
}

/// Typed minimum or maximum aggregation value.
#[derive(Debug, Clone, PartialEq)]
pub enum AggregationMinMax {
    /// Numeric minimum or maximum.
    Number(f64),
    /// Lexicographic minimum or maximum.
    String(String),
    /// Temporal minimum or maximum.
    DateTime(DateTime<Utc>),
}

/// One backend-neutral aggregation bucket for one temporal attribute.
#[derive(Debug, Clone, PartialEq)]
pub struct TemporalAggregationRow {
    /// Expanded attribute IRI.
    pub attribute_name: AttributeName,
    /// NGSI-LD attribute family.
    pub attribute_type: AttributeType,
    /// Multi-attribute discriminator, absent for the default instance.
    pub dataset_id: Option<Uri>,
    /// Start of the aggregation bucket.
    pub bucket_start: Option<DateTime<Utc>>,
    /// End of the aggregation bucket.
    pub bucket_end: Option<DateTime<Utc>>,
    /// Arithmetic mean.
    pub avg: Option<f64>,
    /// Typed minimum.
    pub min: Option<AggregationMinMax>,
    /// Typed maximum.
    pub max: Option<AggregationMinMax>,
    /// Sum.
    pub sum: Option<f64>,
    /// Sum of squares.
    pub sumsq: Option<f64>,
    /// Standard deviation.
    pub stddev: Option<f64>,
    /// Number of observations.
    pub total_count: Option<i64>,
    /// Number of distinct values.
    pub distinct_count: Option<i64>,
}
