//! NGSI-LD Temporal API types (ETSI GS CIM 009 clause 4.11).

use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};
use std::{fmt, str::FromStr};

/// Temporal relationship for time-based filtering (clause 4.11).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimeRel {
    /// time < timeAt (exclusive upper bound)
    Before,
    /// time >= timeAt (inclusive lower bound)
    After,
    /// time >= timeAt AND time < endTimeAt (inclusive start, exclusive end)
    Between,
}

impl FromStr for TimeRel {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        match s.trim() {
            "before" => Ok(Self::Before),
            "after" => Ok(Self::After),
            "between" => Ok(Self::Between),
            other => Err(Error::Validation(format!("Invalid timerel: {}", other))),
        }
    }
}

/// Temporal property to filter on (clause 4.8). Default is ObservedAt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimeProp {
    ObservedAt,
    CreatedAt,
    ModifiedAt,
    DeletedAt,
}

impl FromStr for TimeProp {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        match s.trim() {
            "observedAt" => Ok(Self::ObservedAt),
            "createdAt" => Ok(Self::CreatedAt),
            "modifiedAt" => Ok(Self::ModifiedAt),
            "deletedAt" => Ok(Self::DeletedAt),
            other => Err(Error::Validation(format!("Invalid timeproperty: {}", other))),
        }
    }
}

impl Default for TimeProp {
    fn default() -> Self {
        Self::ObservedAt
    }
}

/// Response format for temporal queries (clause 6.3.12).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TemporalFormat {
    /// Default — full attribute instances
    Normalized,
    /// Simplified [value, timestamp] pairs
    TemporalValues,
    /// Aggregation over time periods
    AggregatedValues,
}

impl Default for TemporalFormat {
    fn default() -> Self {
        Self::Normalized
    }
}

/// Value type for temporal attribute values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum ValueType {
    Number,
    String,
    Boolean,
    Geometry,
    Object,
    DateTime,
    Uri,
}

impl fmt::Display for ValueType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Number => f.write_str("Number"),
            Self::String => f.write_str("String"),
            Self::Boolean => f.write_str("Boolean"),
            Self::Geometry => f.write_str("Geometry"),
            Self::Object => f.write_str("Object"),
            Self::DateTime => f.write_str("DateTime"),
            Self::Uri => f.write_str("Uri"),
        }
    }
}

impl std::str::FromStr for ValueType {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self> {
        match s {
            "Number" => Ok(Self::Number),
            "String" => Ok(Self::String),
            "Boolean" => Ok(Self::Boolean),
            "Geometry" => Ok(Self::Geometry),
            "Object" => Ok(Self::Object),
            "DateTime" => Ok(Self::DateTime),
            "Uri" => Ok(Self::Uri),
            other => Err(Error::Validation(format!("Invalid value type: {}", other))),
        }
    }
}

/// Aggregation methods (clause 4.5.19).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AggrMethod {
    Avg,
    Min,
    Max,
    Sum,
    SumSq,
    StdDev,
    TotalCount,
    DistinctCount,
}

impl FromStr for AggrMethod {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        match s.trim() {
            "avg" => Ok(Self::Avg),
            "min" => Ok(Self::Min),
            "max" => Ok(Self::Max),
            "sum" => Ok(Self::Sum),
            "sumsq" => Ok(Self::SumSq),
            "stddev" => Ok(Self::StdDev),
            "totalCount" => Ok(Self::TotalCount),
            "distinctCount" => Ok(Self::DistinctCount),
            other => Err(Error::Validation(format!("Invalid aggregation method: {}", other))),
        }
    }
}
