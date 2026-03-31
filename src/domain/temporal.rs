//! NGSI-LD temporal types (ETSI GS CIM 009 clause 5.2.4).
//!
//! Types supporting the temporal entity API, which provides access to the
//! history of attribute values over time.

use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use strum::{Display, EnumString};

/// A row from the temporal_entity_attributes table.
#[derive(Debug, Clone, FromRow)]
pub struct TeaEntry {
    #[allow(dead_code)]
    pub id: uuid::Uuid,
    #[allow(dead_code)]
    pub entity_id: String,
    #[allow(dead_code)]
    pub attribute_name: String,
    /// "Property", "Relationship", "GeoProperty", etc.
    pub attribute_type: String,
    pub dataset_id: Option<String>,
    /// "Number", "String", "Boolean", "Geometry", "Object", "DateTime", "Uri"
    pub value_type: String,
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

/// Value type for temporal attribute values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Display, EnumString)]
#[serde(rename_all = "PascalCase")]
#[strum(serialize_all = "PascalCase")]
#[allow(dead_code)]
pub enum ValueType {
    Number,
    String,
    Boolean,
    Geometry,
    Object,
    DateTime,
    Uri,
}

/// Aggregation methods (clause 4.5.19).
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
    use super::*;
    use std::str::FromStr;

    #[test]
    fn time_rel_display_and_parse_round_trip() {
        for variant in [TimeRel::Before, TimeRel::After, TimeRel::Between] {
            let s = variant.to_string();
            let parsed = TimeRel::from_str(&s).unwrap();
            assert_eq!(parsed, variant, "round-trip failed for {variant}");
        }
    }

    #[test]
    fn time_prop_default_is_observed_at() {
        assert_eq!(TimeProp::default(), TimeProp::Observed);
    }

    #[test]
    fn time_prop_display_and_parse_round_trip() {
        for variant in [TimeProp::Observed, TimeProp::Created, TimeProp::Modified, TimeProp::Deleted] {
            let s = variant.to_string();
            let parsed = TimeProp::from_str(&s).unwrap();
            assert_eq!(parsed, variant, "round-trip failed for {variant}");
        }
    }

    #[test]
    fn temporal_format_display_and_parse_round_trip() {
        for variant in [TemporalFormat::Normalized, TemporalFormat::TemporalValues, TemporalFormat::AggregatedValues] {
            let s = variant.to_string();
            let parsed = TemporalFormat::from_str(&s).unwrap();
            assert_eq!(parsed, variant, "round-trip failed for {variant}");
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
            let parsed = AggrMethod::from_str(&s).unwrap();
            assert_eq!(parsed, variant, "round-trip failed for {variant}");
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
            let parsed = ValueType::from_str(&s).unwrap();
            assert_eq!(parsed, variant, "round-trip failed for {variant}");
        }
    }
}
