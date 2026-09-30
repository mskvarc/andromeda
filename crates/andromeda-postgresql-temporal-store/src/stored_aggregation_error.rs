//! What an aggregation row that does not read back as the aggregate it computed
//! looks like.
//!
//! Every column of an aggregation row is computed by the statement itself, so
//! each variant is a statement that answered in a shape it cannot produce, or a
//! computed value outside the broker's documented numeric range. A column that
//! cannot be read at all is a `sqlx` decode failure and is reported as one,
//! naming the column itself.

use andromeda_ngsi_ld_number::{JsonNumber, JsonNumberError};
use andromeda_postgresql_codes::registry::UnknownCompactCode;
use andromeda_unit_catalog::{
    unit_code_id::UnitCodeId,
    unit_code_registry::{UnitCodeSnapshotMalformed, UnitCodeUnknown},
};
use strum::{Display, IntoStaticStr};
use thiserror::Error;

/// A fixed column of an aggregation row.
///
/// Each renders as the alias the aggregation statement projects it under, so
/// the same value names the column in a failure and looks it up in the row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Display, IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
pub enum AggregationColumn {
    /// The period's number of instances.
    AggTotalCount,
    /// The period's number of distinct values.
    AggDistinctCount,
    /// The sum of the period's values.
    AggSum,
    /// The mean of the period's values.
    AggAvg,
    /// The mean of the period's `Time` values.
    AggAvgTime,
    /// The smallest numeric value of the period.
    AggMin,
    /// The largest numeric value of the period.
    AggMax,
    /// The smallest String value of the period.
    AggMinStr,
    /// The largest String value of the period.
    AggMaxStr,
    /// The earliest `DateTime` value of the period.
    AggMinDt,
    /// The latest `DateTime` value of the period.
    AggMaxDt,
    /// The standard deviation of the period's values.
    AggStddev,
    /// The sum of the squares of the period's values.
    AggSumsq,
    /// The lowest attribute family code the group spanned.
    AttributeKindLowest,
    /// The highest attribute family code the group spanned.
    AttributeKindHighest,
    /// The lowest recorded shape code the group spanned.
    RecordedShapeLowest,
    /// The highest recorded shape code the group spanned.
    RecordedShapeHighest,
    /// Whether any of the group's rows carried no unit.
    AnyUnitless,
    /// The start of the period.
    BucketStart,
    /// The end of the period.
    BucketEnd,
}

/// An aggregation row that does not read back as the aggregate it computed.
#[derive(Debug, Error)]
pub enum StoredAggregationError {
    /// A column every non-empty group carries is absent.
    #[error("aggregation {column} is absent from a non-empty group")]
    Absent {
        /// The column that holds no value.
        column: AggregationColumn,
    },
    /// A registry code bound is not a code the registry assigned.
    #[error("aggregation {column} is not a known code: {source}")]
    Code {
        /// The column the code was read from.
        column: AggregationColumn,
        /// The registry's rejection.
        #[source]
        source: UnknownCompactCode,
    },
    /// A decimal aggregate is outside the broker's documented numeric range.
    #[error("aggregation {column} is not a representable number: {source}")]
    Number {
        /// The column the aggregate was read from.
        column: AggregationColumn,
        /// The numeric model's rejection.
        #[source]
        source: JsonNumberError,
    },
    /// A numeric extremum over a boolean attribute counts neither false nor
    /// true.
    #[error("aggregation {column} of a boolean attribute is {counted}, which counts neither false nor true")]
    BooleanExtremum {
        /// The column the extremum was read from.
        column: AggregationColumn,
        /// The number the extremum reported.
        counted: JsonNumber,
    },
    /// The unit-code span reports a bound on one side only, although both
    /// bounds aggregate the same column over the same rows.
    #[error("aggregation unit-code span reports a bound on one side only")]
    OneSidedUnitSpan,
    /// A promoted `unitCode` grouping key produced a group whose unit is not
    /// uniform, which the `GROUP BY` on that unit rules out.
    #[error("a unitCode grouping key produced a non-uniform unit span")]
    NonUniformGroupUnit,
    /// The compiled-in unit catalog snapshot did not parse.
    #[error("the unit catalog is unavailable: {0}")]
    UnitCatalog(#[source] UnitCodeSnapshotMalformed),
    /// A unit identity a grouping key reports names no catalog registration.
    #[error("unitCode grouping key {identity} names no catalog registration: {source}")]
    GroupUnitCode {
        /// The identity the group reported.
        identity: UnitCodeId,
        /// The catalog's rejection.
        #[source]
        source: UnitCodeUnknown,
    },
}

#[cfg(test)]
mod tests {
    use crate::stored_aggregation_error::AggregationColumn;

    #[test]
    fn an_aggregation_column_renders_as_the_alias_its_statement_projects() {
        let projected: &'static str = AggregationColumn::AggSumsq.into();
        pretty_assertions::assert_eq!(projected, "agg_sumsq");
        pretty_assertions::assert_eq!(AggregationColumn::AggMinDt.to_string(), "agg_min_dt");
        pretty_assertions::assert_eq!(AggregationColumn::RecordedShapeHighest.to_string(), "recorded_shape_highest");
    }
}
