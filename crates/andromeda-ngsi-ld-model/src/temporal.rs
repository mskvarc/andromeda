//! Backend-neutral temporal observations and aggregation values.

use andromeda_ngsi_ld_lexicon::{
    attribute_name::AttributeName,
    attribute_type::AttributeType,
    instance_identity::InstanceIdentity,
    time_prop::TimeProp,
    uri::Uri,
};
use andromeda_ngsi_ld_number::JsonNumber;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A single decoded value from temporal history storage.
#[derive(Debug, Clone, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum TemporalValue {
    /// An exact numeric attribute value.
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

#[cfg(test)]
mod tests {
    use crate::temporal::TemporalValue;
    use andromeda_ngsi_ld_number::JsonNumber;
    use std::str::FromStr;

    #[test]
    fn an_exact_number_round_trips_through_the_durable_wire_shape() {
        let value = TemporalValue::Number(JsonNumber::from_str("0.100000000000000005").expect("supported exact number"));
        let encoded = serde_json::to_string(&value).expect("durable JSON");
        pretty_assertions::assert_eq!(encoded, r#"{"number":0.100000000000000005}"#);
        pretty_assertions::assert_eq!(serde_json::from_str::<TemporalValue>(&encoded).expect("round trip"), value);
    }

    #[test]
    fn integer_and_double_payloads_from_old_intents_remain_readable() {
        pretty_assertions::assert_eq!(
            serde_json::from_str::<TemporalValue>(r#"{"number":42}"#).expect("old integer intent"),
            TemporalValue::Number(JsonNumber::from(42))
        );
        pretty_assertions::assert_eq!(
            serde_json::from_str::<TemporalValue>(r#"{"number":1.5}"#).expect("old double intent"),
            TemporalValue::Number(JsonNumber::from_str("1.5").expect("exact decimal"))
        );
    }
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

/// Typed minimum or maximum aggregation value (clause 4.5.19).
///
/// The extremum is reported in the family the observations were recorded in,
/// never in the one the backend happened to compute it through: a boolean
/// aggregated as zero and one comes back a boolean, because `true` is what was
/// observed and `1` is only how it was counted.
#[derive(Debug, Clone, PartialEq)]
pub enum AggregationMinMax {
    /// Numeric minimum or maximum, in the exact shape the stored value has.
    Number(JsonNumber),
    /// Lexicographic minimum or maximum.
    String(String),
    /// Boolean minimum or maximum.
    Boolean(bool),
    /// Temporal minimum or maximum.
    DateTime(DateTime<Utc>),
}

/// One backend-neutral aggregation bucket for one temporal attribute.
///
/// Clause 4.5.19 gives one aggregate value per attribute, `datasetId` and
/// period, so one of these is one period of one attribute instance and nothing
/// finer.
///
/// The arithmetic aggregates are carried as [`JsonNumber`] rather than as
/// doubles: a backend computes them over an exact decimal column, and widening
/// the result to binary floating point here would report a sum or an extremum
/// that was never observed (`docs/exact-numeric-representation.md`).
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
    pub avg: Option<JsonNumber>,
    /// Typed minimum.
    pub min: Option<AggregationMinMax>,
    /// Typed maximum.
    pub max: Option<AggregationMinMax>,
    /// Sum.
    pub sum: Option<JsonNumber>,
    /// Sum of squares.
    pub sumsq: Option<JsonNumber>,
    /// Standard deviation.
    pub stddev: Option<JsonNumber>,
    /// Number of observations.
    pub total_count: Option<i64>,
    /// Number of distinct values.
    pub distinct_count: Option<i64>,
}
