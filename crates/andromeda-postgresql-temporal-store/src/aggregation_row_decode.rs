//! Decoding `sqlx::postgres::PgRow` into the backend-neutral
//! [`TemporalAggregationRow`] and into the span its group covered.
//!
//! Clause 4.5.19 gives one aggregate value per attribute, `datasetId` and
//! period, and the statement groups on exactly those, so one row here is one
//! rendered period. What the rows underneath a group disagreed about is read
//! separately as an [`AggregationGroupSpan`], which decides whether the group is
//! answerable before any aggregate of it is decoded.
//!
//! Nothing here converts a failure into an absent value. An aggregate column the
//! statement did not project is genuinely absent, and that is the only case that
//! yields nothing; a column that is present but unreadable is a typed error,
//! because reporting it as JSON `null` would claim the database computed no
//! aggregate when in truth the broker could not read the one it computed.

use crate::{
    error::TemporalStoreError,
    history_row_decode::{decode_attribute_name, decode_dataset_id, decode_unit_code},
    stored_aggregation_error::{AggregationColumn, StoredAggregationError},
    stored_history_error::UnitCodeColumn,
};
use andromeda_ngsi_ld_lexicon::ngsi_ld_time::NgsiLdTime;
use andromeda_ngsi_ld_model::temporal::{AggregationAverage, AggregationMinMax, TemporalAggregationRow};
use andromeda_ngsi_ld_number::JsonNumber;
use andromeda_postgresql_codes::{attribute_kind::AttributeKind, attribute_kind_code::attribute_type_of, registry::UnknownCompactCode};
use andromeda_postgresql_temporal_query::{
    aggregation_group::{AggregationGroupSpan, CodeSpan, ObservedUnits},
    recorded_shape::RecordedShape,
};
use bigdecimal::BigDecimal;
use chrono::{DateTime, NaiveTime, Utc};
use sqlx::{Decode, Postgres, Row, Type, postgres::PgRow};

/// Read the span of families, value shapes and unit identities one aggregation group covered.
pub(crate) fn decode_aggregation_group_span(row: &PgRow) -> Result<AggregationGroupSpan, TemporalStoreError> {
    Ok(AggregationGroupSpan {
        attribute_name: decode_attribute_name(row)?,
        units: decode_observed_units(row)?,
        families: decode_code_span::<AttributeKind>(row, AggregationColumn::AttributeKindLowest, AggregationColumn::AttributeKindHighest)?,
        value_shapes: decode_code_span::<RecordedShape>(row, AggregationColumn::RecordedShapeLowest, AggregationColumn::RecordedShapeHighest)?,
    })
}

/// Decode one aggregation row into a [`TemporalAggregationRow`].
///
/// The family the period is reported under is the lowest one its rows carried.
/// A group whose rows disagreed about the family survives only when the request
/// counts alone — [`ensure_uniform_aggregation_groups`] refuses every other
/// case — and a count is the same number whichever family it is reported under.
///
/// [`ensure_uniform_aggregation_groups`]: andromeda_postgresql_temporal_query::aggregation_group::ensure_uniform_aggregation_groups
pub(crate) fn decode_aggregation_row(row: &PgRow, span: &AggregationGroupSpan) -> Result<TemporalAggregationRow, TemporalStoreError> {
    let value_shape = span.value_shapes.uniform();
    Ok(TemporalAggregationRow {
        // clone: the decoded row owns the name the span holds for the whole result set.
        attribute_name: span.attribute_name.clone(),
        attribute_type: attribute_type_of(span.families.lowest),
        dataset_id: decode_dataset_id(row)?,
        bucket_start: projected_column(row, AggregationColumn::BucketStart.into())?,
        bucket_end: projected_column(row, AggregationColumn::BucketEnd.into())?,
        avg: decode_average(row, value_shape)?,
        min: decode_extremum(row, value_shape, ExtremumColumns::MIN)?,
        max: decode_extremum(row, value_shape, ExtremumColumns::MAX)?,
        sum: decode_exact_aggregate(row, AggregationColumn::AggSum)?,
        sumsq: decode_exact_aggregate(row, AggregationColumn::AggSumsq)?,
        stddev: decode_exact_aggregate(row, AggregationColumn::AggStddev)?,
        total_count: projected_column(row, AggregationColumn::AggTotalCount.into())?,
        distinct_count: projected_column(row, AggregationColumn::AggDistinctCount.into())?,
        // Populated by the caller, which holds the grouping and the entity the
        // row belongs to; an ungrouped aggregation leaves it empty.
        group_keys: Vec::new(),
    })
}

/// The three columns one extremum is projected into, one per stored value
/// family the extremum can be taken over.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ExtremumColumns {
    /// The `NUMERIC` aggregate.
    numeric: AggregationColumn,
    /// The text aggregate.
    text: AggregationColumn,
    /// The timestamp aggregate.
    instant: AggregationColumn,
}

impl ExtremumColumns {
    /// The columns the period's minimum is projected into.
    const MIN: ExtremumColumns = ExtremumColumns {
        numeric: AggregationColumn::AggMin,
        text: AggregationColumn::AggMinStr,
        instant: AggregationColumn::AggMinDt,
    };
    /// The columns the period's maximum is projected into.
    const MAX: ExtremumColumns = ExtremumColumns {
        numeric: AggregationColumn::AggMax,
        text: AggregationColumn::AggMaxStr,
        instant: AggregationColumn::AggMaxDt,
    };
}

/// Where the extremum of one value shape is read from (clause 4.5.19).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ExtremumColumn {
    /// The `NUMERIC` aggregate, reported as the number it is.
    Numeric,
    /// The `NUMERIC` aggregate, reported as the boolean whose zero or one it counted.
    NumericAsBoolean,
    /// The text aggregate.
    Text,
    /// The timestamp aggregate.
    Instant,
    /// The shape has no ordering, so no extremum of it exists.
    Unordered,
}

/// The column the extremum of a value shape is taken from.
///
/// One arm per column of Tables 4.5.19.1-1, 4.5.19.1-2 and 4.5.19.1-3. A JSON
/// Array reads the numeric aggregate because the table takes its `min` and
/// `max` over "the size of the arrays inside the period", which is what the
/// size-measuring operand computed; a JSON Object has no `min` or `max` at all
/// and so reads nothing. A `Date` and a `Time` read the text aggregate they
/// were stored in, whose canonical fixed-width rendering orders byte-wise in
/// chronological order. A geometry is unordered and a URI is aggregated by
/// count alone, both being N/A for `min` and `max`.
///
/// The value shape alone does not decide a Relationship family, whose extrema
/// Table 4.5.19.1-3 marks N/A whatever column its targets sit in: a
/// `ListRelationship` holds an `objectList` as a JSON array and so reports the
/// Array shape here. The statement is what refuses it — every extremum column
/// is gated on the attribute family, so the aggregate this names is SQL NULL
/// for such a group and no extremum is decoded.
const fn extremum_column(value_shape: RecordedShape) -> ExtremumColumn {
    match value_shape {
        RecordedShape::Number | RecordedShape::Array => ExtremumColumn::Numeric,
        RecordedShape::Boolean => ExtremumColumn::NumericAsBoolean,
        RecordedShape::String | RecordedShape::Date | RecordedShape::Time => ExtremumColumn::Text,
        RecordedShape::DateTime => ExtremumColumn::Instant,
        RecordedShape::Object | RecordedShape::Geometry | RecordedShape::Uri => ExtremumColumn::Unordered,
    }
}

/// Read a typed extremum from the column the group's value shape names.
///
/// A group with no single value shape has no extremum to read: it survives
/// validation only for counting methods, and those project no extremum column.
fn decode_extremum(row: &PgRow, value_shape: Option<RecordedShape>, columns: ExtremumColumns) -> Result<Option<AggregationMinMax>, TemporalStoreError> {
    let Some(value_shape) = value_shape else {
        return Ok(None);
    };
    match extremum_column(value_shape) {
        ExtremumColumn::Numeric => Ok(decode_exact_aggregate(row, columns.numeric)?.map(AggregationMinMax::Number)),
        ExtremumColumn::NumericAsBoolean => Ok(decode_exact_aggregate(row, columns.numeric)?
            .map(|counted| boolean_extremum(counted, columns.numeric))
            .transpose()?),
        ExtremumColumn::Text => Ok(projected_column::<String>(row, columns.text.into())?.map(AggregationMinMax::String)),
        ExtremumColumn::Instant => Ok(projected_column::<DateTime<Utc>>(row, columns.instant.into())?.map(AggregationMinMax::DateTime)),
        ExtremumColumn::Unordered => Ok(None),
    }
}

/// The boolean whose count produced a numeric extremum.
///
/// The unified numeric expression counts a boolean as zero or one, so the
/// extremum arrives as one of those two numbers and is reported in the family
/// the observations were recorded in. Any other number means the row did not
/// come from that expression, which is a fault rather than a value.
fn boolean_extremum(counted: JsonNumber, column: AggregationColumn) -> Result<AggregationMinMax, StoredAggregationError> {
    if counted == JsonNumber::from(0) {
        Ok(AggregationMinMax::Boolean(false))
    } else if counted == JsonNumber::from(1) {
        Ok(AggregationMinMax::Boolean(true))
    } else {
        Err(StoredAggregationError::BooleanExtremum { column, counted })
    }
}

/// Read the period's mean from the column its recorded shape names.
///
/// Table 4.5.19.1-1 makes `avg` a number for the JSON Number, JSON Boolean and
/// JSON Array columns, while Table 4.5.19.1-2 makes it "the average time inside
/// the period" for the Time column. Both are projected and the shape decides,
/// the same way the three extremum columns are read.
fn decode_average(row: &PgRow, value_shape: Option<RecordedShape>) -> Result<Option<AggregationAverage>, TemporalStoreError> {
    if value_shape == Some(RecordedShape::Time) {
        return match projected_column::<NaiveTime>(row, AggregationColumn::AggAvgTime.into())? {
            Some(mean) => Ok(Some(AggregationAverage::Time(
                NgsiLdTime::normalise(mean).map_err(TemporalStoreError::StoredTime)?,
            ))),
            None => Ok(None),
        };
    }
    Ok(decode_exact_aggregate(row, AggregationColumn::AggAvg)?.map(AggregationAverage::Number))
}

/// Read one `NUMERIC` aggregate as the exact JSON number it is.
fn decode_exact_aggregate(row: &PgRow, column: AggregationColumn) -> Result<Option<JsonNumber>, TemporalStoreError> {
    match projected_column::<BigDecimal>(row, column.into())? {
        Some(decimal) => Ok(Some(exact_aggregate(&decimal, column)?)),
        None => Ok(None),
    }
}

/// The JSON number a `NUMERIC` aggregate names, reported against its column.
///
/// Every arithmetic aggregate PostgreSQL computes over `value_number` is
/// `NUMERIC`, so it is converted directly into the JSON numeric model the value
/// arrived in: an integer beyond 2^53
/// comes back as that integer, and a literal the model cannot carry fails the
/// decode instead of becoming a rounded value or a null. The failure names the
/// column because one row carries several decimal aggregates and the literal
/// alone does not say which one was unreadable.
fn exact_aggregate(decimal: &BigDecimal, column: AggregationColumn) -> Result<JsonNumber, StoredAggregationError> {
    JsonNumber::from_decimal(decimal).map_err(|source| StoredAggregationError::Number { column, source })
}

/// Read a column the statement projects only for the methods that need it.
///
/// A column the statement never selected is absent, which is what the requested
/// method list says it should be. A column that is present and unreadable is an
/// error: the alternative is reporting a computed aggregate as no aggregate at
/// all.
pub(crate) fn projected_column<'r, T>(row: &'r PgRow, column: &str) -> Result<Option<T>, TemporalStoreError>
where
    T: Decode<'r, Postgres> + Type<Postgres>,
{
    match row.try_column(column) {
        Err(_) => Ok(None),
        Ok(_) => Ok(row.try_get::<Option<T>, _>(column)?),
    }
}

/// Read the two registry-code bounds one aggregation group spanned.
fn decode_code_span<Code>(row: &PgRow, lowest_column: AggregationColumn, highest_column: AggregationColumn) -> Result<CodeSpan<Code>, TemporalStoreError>
where
    Code: TryFrom<i16, Error = UnknownCompactCode>,
{
    Ok(CodeSpan {
        lowest: decode_code(row, lowest_column)?,
        highest: decode_code(row, highest_column)?,
    })
}

/// Read one registry code an aggregate reported for a group.
///
/// The bound is null only for an empty group, which the statement cannot
/// produce: a group exists because at least one row fell into it.
fn decode_code<Code>(row: &PgRow, column: AggregationColumn) -> Result<Code, TemporalStoreError>
where
    Code: TryFrom<i16, Error = UnknownCompactCode>,
{
    let stored: Option<i16> = row.try_get(<&'static str>::from(column))?;
    let stored = stored.ok_or(StoredAggregationError::Absent { column })?;
    Ok(Code::try_from(stored).map_err(|source| StoredAggregationError::Code { column, source })?)
}

/// Read the unit codes one aggregation group's rows carried.
///
/// `MIN` and `MAX` skip nulls, so the two bounds alone cannot tell a group whose
/// rows all carried a unit from one that also held unannotated rows; the null
/// test projected beside them is what separates the two.
fn decode_observed_units(row: &PgRow) -> Result<ObservedUnits, TemporalStoreError> {
    let lowest = decode_unit_code(row, UnitCodeColumn::UnitCodeIdLowest)?;
    let highest = decode_unit_code(row, UnitCodeColumn::UnitCodeIdHighest)?;
    let any_unitless: Option<bool> = row.try_get(<&'static str>::from(AggregationColumn::AnyUnitless))?;
    let any_unitless = any_unitless.ok_or(StoredAggregationError::Absent {
        column: AggregationColumn::AnyUnitless,
    })?;

    match (lowest, highest) {
        (None, None) => Ok(ObservedUnits::Uniform(None)),
        (Some(first), Some(second)) if first != second => Ok(ObservedUnits::Conflicting { first, second }),
        (Some(present), Some(_)) => Ok(if any_unitless {
            ObservedUnits::UnitAndUnitless { present }
        } else {
            ObservedUnits::Uniform(Some(present))
        }),
        // Both bounds are aggregates of the same column over the same rows, so
        // one being null while the other is not is corrupt rather than a case
        // to resolve by precedence.
        (Some(_), None) | (None, Some(_)) => Err(StoredAggregationError::OneSidedUnitSpan.into()),
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        aggregation_row_decode::{ExtremumColumn, boolean_extremum, exact_aggregate, extremum_column},
        stored_aggregation_error::{AggregationColumn, StoredAggregationError},
    };
    use andromeda_ngsi_ld_model::temporal::AggregationMinMax;
    use andromeda_ngsi_ld_number::JsonNumber;
    use andromeda_postgresql_temporal_query::recorded_shape::RecordedShape;
    use bigdecimal::BigDecimal;
    use std::str::FromStr;

    fn decimal(literal: &str) -> BigDecimal {
        BigDecimal::from_str(literal).expect("a decimal literal")
    }

    /// The extremum is read from the column its value shape names, and a shape
    /// with no ordering has none at all. Enumerated over every shape the
    /// projection can report so one added later cannot silently inherit the
    /// numeric column.
    #[test]
    fn every_value_shape_names_the_column_its_extremum_lives_in() {
        let expected = [
            (RecordedShape::Number, ExtremumColumn::Numeric),
            (RecordedShape::Array, ExtremumColumn::Numeric),
            (RecordedShape::Boolean, ExtremumColumn::NumericAsBoolean),
            (RecordedShape::String, ExtremumColumn::Text),
            (RecordedShape::Date, ExtremumColumn::Text),
            (RecordedShape::Time, ExtremumColumn::Text),
            (RecordedShape::DateTime, ExtremumColumn::Instant),
            (RecordedShape::Object, ExtremumColumn::Unordered),
            (RecordedShape::Geometry, ExtremumColumn::Unordered),
            (RecordedShape::Uri, ExtremumColumn::Unordered),
        ];
        pretty_assertions::assert_eq!(expected.len(), RecordedShape::ALL.len(), "a value shape is missing from the mapping");
        for (value_shape, column) in expected {
            pretty_assertions::assert_eq!(extremum_column(value_shape), column, "{value_shape:?}");
        }
    }

    /// Table 4.5.19.1-1, "JSON Object" column: an object has no `min` and no
    /// `max`, so it is reported as no extremum rather than as the length the
    /// size-measuring operand would have produced for an array beside it.
    #[test]
    fn an_object_has_no_extremum_rather_than_a_length() {
        pretty_assertions::assert_eq!(extremum_column(RecordedShape::Object), ExtremumColumn::Unordered);
    }

    /// Table 4.5.19.1-1, "JSON Array" column: `min` and `max` are the minimum
    /// and maximum size of the arrays inside the period, which is the number
    /// the size-measuring operand aggregated.
    #[test]
    fn an_array_reports_the_extreme_size_the_numeric_aggregate_measured() {
        pretty_assertions::assert_eq!(extremum_column(RecordedShape::Array), ExtremumColumn::Numeric);
    }

    #[test]
    fn a_boolean_extremum_comes_back_as_the_boolean_its_count_stood_for() {
        pretty_assertions::assert_eq!(
            boolean_extremum(JsonNumber::from(0), AggregationColumn::AggMin).expect("zero counts false"),
            AggregationMinMax::Boolean(false)
        );
        pretty_assertions::assert_eq!(
            boolean_extremum(JsonNumber::from(1), AggregationColumn::AggMax).expect("one counts true"),
            AggregationMinMax::Boolean(true)
        );
    }

    /// A number that counts neither `false` nor `true` did not come from the
    /// boolean branch of the numeric expression, so it is refused instead of
    /// being reported as one of the two.
    #[test]
    fn a_boolean_extremum_that_counts_neither_value_is_refused() {
        let error = boolean_extremum(JsonNumber::from(2), AggregationColumn::AggMin).expect_err("two counts nothing");
        assert!(
            matches!(
                error,
                StoredAggregationError::BooleanExtremum {
                    column: AggregationColumn::AggMin,
                    ..
                }
            ),
            "got {error}"
        );
        assert!(boolean_extremum("0.5".parse::<JsonNumber>().expect("an exact decimal"), AggregationColumn::AggMin).is_err());
    }

    /// A `SUM` over integers is an integer, and one beyond 2^53 has no exact
    /// double: reading it through the JSON numeric model keeps it whole.
    #[test]
    fn a_sum_beyond_the_exact_double_range_keeps_its_integer_shape() {
        let number = exact_aggregate(&decimal("9007199254740993"), AggregationColumn::AggSum).expect("an integer aggregate");
        pretty_assertions::assert_eq!(number, JsonNumber::from(9_007_199_254_740_993_i64));
        pretty_assertions::assert_eq!(serde_json::to_string(&number).expect("a JSON number"), "9007199254740993");
    }

    /// An integer above the signed range is still an integer rather than the
    /// double nearest to it.
    #[test]
    fn an_integer_above_the_signed_range_keeps_its_unsigned_shape() {
        let number = exact_aggregate(&decimal("18446744073709551615"), AggregationColumn::AggSum).expect("an integer aggregate");
        pretty_assertions::assert_eq!(number, JsonNumber::from(u64::MAX));
    }

    /// An average is generally fractional, and the fractional part survives the
    /// decode rather than being truncated to the integer around it.
    #[test]
    fn a_fractional_average_keeps_its_fraction() {
        let number = exact_aggregate(&decimal("2.5"), AggregationColumn::AggAvg).expect("a fractional aggregate");
        pretty_assertions::assert_eq!(number, "2.5".parse::<JsonNumber>().expect("an exact decimal"));
    }

    #[test]
    fn a_negative_aggregate_keeps_its_sign() {
        let number = exact_aggregate(&decimal("-273.15"), AggregationColumn::AggMin).expect("a negative aggregate");
        pretty_assertions::assert_eq!(number, "-273.15".parse::<JsonNumber>().expect("an exact decimal"));
    }

    /// A zero-valued aggregate -- the standard deviation of a single
    /// observation, for one -- is a number like any other, whatever scale the
    /// column reported it at.
    #[test]
    fn a_scaled_zero_is_a_number_like_any_other() {
        let number = exact_aggregate(&decimal("0.000"), AggregationColumn::AggStddev).expect("a zero aggregate");
        pretty_assertions::assert_eq!(number, JsonNumber::from(0));
        pretty_assertions::assert_eq!(serde_json::to_string(&number).expect("a JSON number"), "0");
    }

    /// The failure names the column, because one aggregation row carries
    /// several decimal aggregates and the literal alone does not say which one
    /// was unreadable.
    #[test]
    fn a_decode_failure_names_the_column_it_came_from() {
        let error = exact_aggregate(&decimal("1e131072"), AggregationColumn::AggSumsq).expect_err("an unrepresentable magnitude is refused");
        assert!(
            matches!(
                error,
                StoredAggregationError::Number {
                    column: AggregationColumn::AggSumsq,
                    ..
                }
            ),
            "got {error}"
        );
        assert!(error.to_string().contains("agg_sumsq"), "got {error}");
    }
}
