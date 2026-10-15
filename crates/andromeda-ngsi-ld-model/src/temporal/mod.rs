//! Backend-neutral temporal observations and aggregation values.

use crate::temporal::{observation_metadata::ObservationMetadata, structured_value::StructuredValue};
use andromeda_ngsi_ld_entity::geometry_value::GeometryValue;
use andromeda_ngsi_ld_lexicon::{
    attribute_name::AttributeName,
    attribute_type::AttributeType,
    identifier::Identifier,
    instance_identity::InstanceIdentity,
    ngsi_ld_time::NgsiLdTime,
    scope::path::ScopePath,
    time_prop::TimeProp,
    uri::Uri,
};
use andromeda_ngsi_ld_number::JsonNumber;
use andromeda_ngsi_ld_vocabulary::NULL_URI;
use chrono::{DateTime, Utc};

/// Reading the aggregated representation a peer answered with.
pub mod aggregated_decoding;
/// The aggregated representation of the Temporal Evolution of an Entity.
pub mod aggregated_entity;
/// The members an observed instance carries beside its value.
pub mod observation_metadata;
/// Why a simplified temporal attribute is refused.
pub mod plural_member_refusal;
/// The Temporal Evolutions a context source answered with.
pub mod received_temporal;
/// Unfolding the simplified temporal representation while a body is read.
mod simplified_unfold;
/// The structured value of one observation and the JSON a history row stores it as.
pub mod structured_value;
/// Reading and writing the stored JSON of a structured value.
pub mod structured_value_codec;
#[cfg(test)]
mod structured_value_tests;
/// The instances of one attribute of a Temporal Evolution.
pub mod temporal_attribute;
/// The Temporal Evolution of one entity in an answer.
pub mod temporal_body;
/// The expanded Temporal Evolution of an Entity.
pub mod temporal_document;
/// The Temporal Evolution of an Entity, compact and normalized.
pub mod temporal_entity;
/// The history of one entity as the operations gather it.
pub mod temporal_history;
/// The Temporal Evolution a client writes.
pub mod written_temporal_entity;

/// A single decoded value from temporal history storage.
///
/// Decoding a structured value needs the attribute family the row records, so
/// a value is decoded as part of its [`TemporalHistoryRow`] rather than alone.
#[derive(Debug, Clone, PartialEq)]
pub enum TemporalValue {
    /// An exact numeric attribute value.
    Number(JsonNumber),
    /// A string attribute value.
    String(String),
    /// A boolean attribute value.
    Boolean(bool),
    /// A `GeoJSON` geometry, held decoded with its `bbox` and foreign members.
    Geometry(GeometryValue),
    /// A structured attribute value: what no typed column can hold.
    ///
    /// What the value is depends on the attribute family that carries it
    /// (clause 4.5): the values of a Property, the targets of a Relationship
    /// naming several, a language map, a JSON literal, a list.
    Structured(StructuredValue),
    /// A datetime attribute value.
    DateTime(DateTime<Utc>),
    /// A URI attribute value.
    Uri(Uri),
    /// The Scopes one reified scope instance names (clause 4.18).
    ///
    /// Clause 4.18 gives the Scope a value shape of its own — "The value of
    /// scope is represented as a JSON array in case there is more than one
    /// Scope" — and no attribute value has that shape, so it is its own variant
    /// rather than a structured value: a JSON-LD value node cannot hold an
    /// array at all, and several Scopes are several value nodes. The paths are
    /// strongly typed for the same reason storage holds them as label paths — a
    /// Scope is a validated clause 4.18 path, never free text.
    Scopes(Vec<ScopePath>),
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
    #[must_use]
    pub const fn deleted_at(&self) -> Option<DateTime<Utc>> {
        match self {
            TemporalValue::Tombstone { deleted_at } => Some(*deleted_at),
            TemporalValue::Number(_)
            | TemporalValue::String(_)
            | TemporalValue::Boolean(_)
            | TemporalValue::Geometry(_)
            | TemporalValue::Structured(_)
            | TemporalValue::DateTime(_)
            | TemporalValue::Uri(_)
            | TemporalValue::Scopes(_) => None,
        }
    }
}

/// One decoded temporal attribute observation.
#[derive(Debug, Clone, PartialEq)]
pub struct TemporalHistoryRow {
    /// Expanded attribute IRI.
    pub attribute_name: AttributeName,
    /// NGSI-LD attribute family.
    pub attribute_type: AttributeType,
    /// Multi-attribute discriminator, absent for the default instance.
    pub dataset_id: Option<Identifier>,
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
    /// The members the instance carries beside its value: its sub-attributes
    /// and the companion members of its family.
    pub metadata: Option<ObservationMetadata>,
    /// Whether the page the row was read in left instances of its stream out
    /// (clause 6.3.10).
    pub stream_extent: StreamExtent,
}

/// How much of one attribute stream a paged temporal read returned
/// (clause 6.3.10).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StreamExtent {
    /// Every instance of the stream in the window: the read was not paged, or
    /// the page held them all.
    #[default]
    Whole,
    /// The stream holds instances in the window beyond the page.
    Cut,
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
    #[must_use]
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

/// The arithmetic mean of one aggregation period.
///
/// Table 4.5.19.1-1 defines `avg` over numbers, booleans and array sizes as a
/// number, but Table 4.5.19.1-2 defines it over a `Time` as "the average time
/// inside the period" -- a time of day, not a count. Carrying both in one type
/// is what lets the renderer emit each in its own lexical form instead of
/// forcing a time through a numeric member.
#[derive(Debug, Clone, PartialEq)]
pub enum AggregationAverage {
    /// A numeric mean, in the exact shape the stored values have.
    Number(JsonNumber),
    /// The mean time of day, in the clause 4.6.3 `Time` rendering.
    Time(NgsiLdTime),
}

/// One rendered group-key value of a grouped aggregation row (`aggrGroupBy`
/// extension).
///
/// A grouped aggregation nests its triples under one map key per grouping term.
/// The three shapes are the map keys of the grouping extension: `@none` for a row whose
/// key was absent, `urn:ngsi-ld:null` for an explicit NGSI-LD Null value key,
/// and the rendered value otherwise. The value is carried as text because it is
/// a JSON object key, which is unstructured by construction.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum GroupKeyValue {
    /// The key was absent for this row.
    Absent,
    /// The key was an explicit NGSI-LD Null value.
    Null,
    /// The rendered group value used as the nesting map key.
    Value(String),
}

impl GroupKeyValue {
    /// The `@none` map key of an absent group value.
    pub const NONE_KEY: &'static str = "@none";

    /// The map key of an explicit NGSI-LD Null: the clause 4.5.0
    /// `urn:ngsi-ld:null` sentinel, taken from the validated vocabulary constant
    /// rather than spelled as a raw string.
    #[must_use]
    pub fn null_key() -> &'static str {
        NULL_URI.as_str()
    }

    /// The JSON object key this group value nests its triples under.
    #[must_use]
    pub fn as_map_key(&self) -> &str {
        match self {
            GroupKeyValue::Absent => GroupKeyValue::NONE_KEY,
            GroupKeyValue::Null => GroupKeyValue::null_key(),
            GroupKeyValue::Value(value) => value,
        }
    }
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
/// that was never observed.
#[derive(Debug, Clone, PartialEq)]
pub struct TemporalAggregationRow {
    /// Expanded attribute IRI.
    pub attribute_name: AttributeName,
    /// NGSI-LD attribute family.
    pub attribute_type: AttributeType,
    /// Multi-attribute discriminator, absent for the default instance.
    pub dataset_id: Option<Identifier>,
    /// Start of the aggregation bucket. `None` only for the whole-range period
    /// of a query that sets no opening instant.
    pub bucket_start: Option<DateTime<Utc>>,
    /// End of the aggregation bucket. `None` only for the whole-range period
    /// of a query that sets no closing instant.
    pub bucket_end: Option<DateTime<Utc>>,
    /// Arithmetic mean, or the mean time of day for a `Time`-valued Property.
    pub avg: Option<AggregationAverage>,
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
    /// The rendered group-key values this row falls under, one per grouping term
    /// in order (`aggrGroupBy` extension). Empty for an ungrouped aggregation.
    pub group_keys: Vec<GroupKeyValue>,
}
