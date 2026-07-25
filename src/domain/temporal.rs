//! NGSI-LD temporal types (ETSI GS CIM 009 clause 5.2.21).
//!
//! Types supporting the temporal entity API, which provides access to the
//! history of attribute values over time.

use crate::domain::attribute_type::AttributeType;
use derive_more::From;
use iri_rs::IriBuf;
use strum::{Display, EnumString};

/// Dictionary key for an expanded attribute IRI in the `attribute_names` table.
///
/// `attribute_names` normalises attribute IRIs to a `SMALLINT` so the temporal
/// catalogue, the history hypertable, and the `attribute_current` projection can
/// all index on a two-byte key instead of a full IRI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, From)]
pub struct AttributeNameId(i16);

impl AttributeNameId {
    /// The underlying `SMALLINT` value, for SQL binding.
    pub const fn get(self) -> i16 {
        self.0
    }
}

/// A typed row from the temporal_entity_attributes table.
#[derive(Debug, Clone)]
pub struct TeaEntry {
    pub attribute_name: IriBuf,
    pub attribute_type: AttributeType,
    /// Multi-attribute discriminator (NGSI-LD 5.2.5).
    ///
    /// Typed as a URI rather than a URN: the specification constrains
    /// `datasetId` to a URI, and plain-URI dataset identifiers are legal. The
    /// previous `Urn` typing silently discarded them, collapsing distinct
    /// instances onto the default one.
    pub dataset_id: Option<IriBuf>,
    pub value_type: ValueType,
}

// ---------------------------------------------------------------------------
// Temporal API types (ETSI GS CIM 009 clause 4.11)
// ---------------------------------------------------------------------------

/// Temporal relationship for time-based filtering (clause 4.11).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Display, EnumString)]
#[strum(serialize_all = "camelCase")]
pub enum TimeRel {
    /// time < timeAt (exclusive upper bound).
    Before,
    /// time >= timeAt (inclusive lower bound).
    After,
    /// time >= timeAt AND time < endTimeAt (inclusive start, exclusive end).
    Between,
}

/// Temporal property to filter on (clause 4.8).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Display, EnumString)]
#[strum(serialize_all = "camelCase")]
pub enum TimeProp {
    /// Select by observedAt timestamp (default).
    #[default]
    #[strum(serialize = "observedAt")]
    Observed,
    /// Select by createdAt timestamp.
    #[strum(serialize = "createdAt")]
    Created,
    /// Select by modifiedAt timestamp.
    #[strum(serialize = "modifiedAt")]
    Modified,
    /// Select by deletedAt timestamp.
    #[strum(serialize = "deletedAt")]
    Deleted,
}

/// Response format for temporal queries (clause 6.3.12).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Display, EnumString)]
#[strum(serialize_all = "camelCase")]
pub enum TemporalFormat {
    /// Default normalized temporal representation.
    #[default]
    Normalized,
    /// Temporal values representation with compact arrays.
    TemporalValues,
    /// Aggregated values representation with statistical summaries.
    AggregatedValues,
}

/// Typed value discriminator for temporal attribute values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Display, EnumString)]
#[strum(serialize_all = "PascalCase")]
pub enum ValueType {
    Number,
    String,
    Boolean,
    Geometry,
    Object,
    DateTime,
    Uri,
}

impl ValueType {
    /// Returns the canonical string representation used in persistence.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Number => "Number",
            Self::String => "String",
            Self::Boolean => "Boolean",
            Self::Geometry => "Geometry",
            Self::Object => "Object",
            Self::DateTime => "DateTime",
            Self::Uri => "Uri",
        }
    }
}

/// Aggregation methods for temporal queries (clause 4.5.19).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Display, EnumString)]
#[strum(serialize_all = "camelCase")]
pub enum AggrMethod {
    /// Arithmetic mean.
    Avg,
    /// Minimum value.
    Min,
    /// Maximum value.
    Max,
    /// Sum of numeric values.
    Sum,
    /// Sum of squares.
    #[strum(serialize = "sumsq")]
    SumSq,
    /// Standard deviation.
    #[strum(serialize = "stddev")]
    StdDev,
    /// Total count of values.
    TotalCount,
    /// Count of distinct values.
    DistinctCount,
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]
    use super::*;
    use std::str::FromStr;

    #[test]
    fn time_rel_display_and_parse_round_trip() {
        for variant in [TimeRel::Before, TimeRel::After, TimeRel::Between] {
            let s = variant.to_string();
            let parsed = TimeRel::from_str(&s).expect("parse timeRel");
            pretty_assertions::assert_eq!(parsed, variant, "round-trip failed for {variant}");
        }
    }

    #[test]
    fn time_prop_default_is_observed_at() {
        pretty_assertions::assert_eq!(TimeProp::default(), TimeProp::Observed);
    }

    #[test]
    fn time_prop_display_and_parse_round_trip() {
        for variant in [TimeProp::Observed, TimeProp::Created, TimeProp::Modified, TimeProp::Deleted] {
            let s = variant.to_string();
            let parsed = TimeProp::from_str(&s).expect("parse timeProperty");
            pretty_assertions::assert_eq!(parsed, variant, "round-trip failed for {variant}");
        }
    }

    #[test]
    fn temporal_format_display_and_parse_round_trip() {
        for variant in [TemporalFormat::Normalized, TemporalFormat::TemporalValues, TemporalFormat::AggregatedValues] {
            let s = variant.to_string();
            let parsed = TemporalFormat::from_str(&s).expect("parse temporal format");
            pretty_assertions::assert_eq!(parsed, variant, "round-trip failed for {variant}");
        }
    }

    #[test]
    fn aggr_method_display_and_parse_round_trip() {
        for variant in [
            AggrMethod::Avg,
            AggrMethod::Min,
            AggrMethod::Max,
            AggrMethod::Sum,
            AggrMethod::SumSq,
            AggrMethod::StdDev,
            AggrMethod::TotalCount,
            AggrMethod::DistinctCount,
        ] {
            let s = variant.to_string();
            let parsed = AggrMethod::from_str(&s).expect("parse aggrMethod");
            pretty_assertions::assert_eq!(parsed, variant, "round-trip failed for {variant}");
        }
    }

    #[test]
    fn value_type_display_and_parse_round_trip() {
        for variant in [
            ValueType::Number,
            ValueType::String,
            ValueType::Boolean,
            ValueType::Geometry,
            ValueType::Object,
            ValueType::DateTime,
            ValueType::Uri,
        ] {
            let s = variant.to_string();
            let parsed = ValueType::from_str(&s).expect("parse valueType");
            pretty_assertions::assert_eq!(parsed, variant, "round-trip failed for {variant}");
        }
    }
}
