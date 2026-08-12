//! Backend-neutral temporal observations and aggregation values.

use crate::attribute::json_number::JsonNumber;
use andromeda_ngsi_ld_lexicon::{
    attribute_name::AttributeName,
    attribute_type::AttributeType,
    instance_identity::InstanceIdentity,
    time_prop::TimeProp,
    uri::Uri,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A single decoded value from temporal history storage.
#[derive(Debug, Clone, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum TemporalValue {
    /// A numeric attribute value, in the shape it was parsed in.
    ///
    /// The shape survives the whole write path so an integer the double format
    /// cannot name reaches the arbitrary-precision storage column unchanged
    /// instead of being rounded at the first conversion.
    Number(JsonNumber),
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
    /// A deletion marker observation, carrying the instant it deleted at.
    ///
    /// Clauses 4.5.7 and 4.5.8 give a deleted instance a `deletedAt` and the
    /// `urn:ngsi-ld:null` value in place of a typed one. The null URI is
    /// synthesised at rendering from this variant, never stored, and a
    /// deletion instant is inseparable from the deletion it marks — holding it
    /// here is what stops a tombstone without one, or a live observation
    /// carrying one, from being expressible.
    Tombstone {
        /// The instant the instance was deleted at.
        deleted_at: DateTime<Utc>,
    },
}

impl TemporalValue {
    /// The deletion instant this observation carries, which only a tombstone has.
    pub const fn deleted_at(&self) -> Option<DateTime<Utc>> {
        match self {
            TemporalValue::Tombstone { deleted_at } => Some(*deleted_at),
            TemporalValue::Number(_)
            | TemporalValue::String(_)
            | TemporalValue::Boolean(_)
            | TemporalValue::Geometry(_)
            | TemporalValue::Object(_)
            | TemporalValue::DateTime(_)
            | TemporalValue::Uri(_) => None,
        }
    }
}

/// One decoded temporal attribute observation.
#[derive(Debug, Clone, Deserialize, PartialEq, Serialize)]
pub struct TemporalHistoryRow {
    /// Expanded attribute IRI.
    pub attribute_name: AttributeName,
    /// NGSI-LD attribute family.
    pub attribute_type: AttributeType,
    /// Multi-attribute discriminator, absent for the default instance.
    pub dataset_id: Option<Uri>,
    /// Decoded observation value.
    pub value: TemporalValue,
    /// Client-declared observation instant, absent when the client declared none.
    pub observed_at: Option<DateTime<Utc>>,
    /// The instant the broker recorded this observation at.
    ///
    /// This is the operational write clock the store orders and partitions on.
    /// Clause 5.2.21 admits only `observedAt`, `createdAt`, `modifiedAt` and
    /// `deletedAt` as Temporal Properties, so this instant is never rendered
    /// and never stands in for one of them.
    pub recorded_at: DateTime<Utc>,
    /// The instant this instance was created at, absent when unrecorded.
    pub created_at: Option<DateTime<Utc>>,
    /// The instant this instance was last modified at, absent when unrecorded.
    pub modified_at: Option<DateTime<Utc>>,
    /// Stable temporal instance identifier, in the form it is held in.
    pub instance: InstanceIdentity,
    /// Additional expanded metadata attached to the instance.
    pub metadata: Option<Value>,
    /// Total observations in the attribute partition when requested by the query.
    pub partition_total: Option<i64>,
}

impl TemporalHistoryRow {
    /// The instant this observation carries on one of the four time axes.
    ///
    /// Clause 4.11 defaults a temporal query's selector to `observedAt` and
    /// clause 5.2.21 admits `createdAt`, `modifiedAt` and `deletedAt` beside
    /// it. Each axis answers from its own value alone: an observation that
    /// declared no `observedAt` matches no `observedAt` window, and
    /// [`recorded_at`](TemporalHistoryRow::recorded_at) never fills a missing
    /// axis in — doing so would invent a match on a Temporal Property the
    /// observation does not have.
    pub const fn time_axis(&self, timeproperty: TimeProp) -> Option<DateTime<Utc>> {
        match timeproperty {
            TimeProp::Observed => self.observed_at,
            TimeProp::Created => self.created_at,
            TimeProp::Modified => self.modified_at,
            TimeProp::Deleted => self.value.deleted_at(),
        }
    }
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
