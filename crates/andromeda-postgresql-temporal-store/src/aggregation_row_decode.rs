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
    attribute_kind_code::attribute_type_of,
    error::TemporalStoreError,
    history_row_decode::{decode_attribute_name, decode_dataset_id, decode_unit_code_id},
    stored_decimal::json_number_of,
};
use andromeda_ngsi_ld_model::{
    attribute::json_number::JsonNumber,
    temporal::{AggregationMinMax, TemporalAggregationRow},
};
use andromeda_postgresql_codes::{attribute_kind::AttributeKind, registry::UnknownCompactCode, value_kind::ValueKind};
use andromeda_postgresql_temporal_query::aggregation_group::{AggregationGroupSpan, CodeSpan, ObservedUnits};
use chrono::{DateTime, Utc};
use sqlx::{Decode, Postgres, Row, Type, postgres::PgRow, types::BigDecimal};

/// Read the span of families, value shapes and unit identities one aggregation group covered.
pub(crate) fn decode_aggregation_group_span(row: &PgRow) -> Result<AggregationGroupSpan, TemporalStoreError> {
    Ok(AggregationGroupSpan {
        attribute_name: decode_attribute_name(row)?,
        units: decode_observed_units(row)?,
        families: decode_code_span::<AttributeKind>(row, "attribute_kind_lowest", "attribute_kind_highest")?,
        value_shapes: decode_code_span::<ValueKind>(row, "value_kind_lowest", "value_kind_highest")?,
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
        bucket_start: projected_column(row, "bucket_start")?,
        bucket_end: projected_column(row, "bucket_end")?,
        avg: decode_exact_aggregate(row, "agg_avg")?,
        min: decode_extremum(row, value_shape, "agg_min", "agg_min_str", "agg_min_dt")?,
        max: decode_extremum(row, value_shape, "agg_max", "agg_max_str", "agg_max_dt")?,
        sum: decode_exact_aggregate(row, "agg_sum")?,
        sumsq: decode_exact_aggregate(row, "agg_sumsq")?,
        stddev: decode_exact_aggregate(row, "agg_stddev")?,
        total_count: projected_column(row, "agg_total_count")?,
        distinct_count: projected_column(row, "agg_distinct_count")?,
    })
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
/// Only shapes with an ordering have one. The unified numeric expression
/// measures an array's length, and the longest array is not the greatest value,
/// so an object or array has no extremum rather than the length of one; a
/// geometry is likewise unordered, a URI is aggregated by count alone, and a
/// tombstone carries no value at all (clause 4.5.7).
const fn extremum_column(value_shape: ValueKind) -> ExtremumColumn {
    match value_shape {
        ValueKind::Number => ExtremumColumn::Numeric,
        ValueKind::Boolean => ExtremumColumn::NumericAsBoolean,
        ValueKind::String => ExtremumColumn::Text,
        ValueKind::DateTime => ExtremumColumn::Instant,
        ValueKind::Object | ValueKind::Geometry | ValueKind::Uri | ValueKind::Tombstone => ExtremumColumn::Unordered,
    }
}

/// Read a typed extremum from the column the group's value shape names.
///
/// A group with no single value shape has no extremum to read: it survives
/// validation only for counting methods, and those project no extremum column.
fn decode_extremum(
    row: &PgRow,
    value_shape: Option<ValueKind>,
    numeric_column: &str,
    text_column: &str,
    instant_column: &str,
) -> Result<Option<AggregationMinMax>, TemporalStoreError> {
    let Some(value_shape) = value_shape else {
        return Ok(None);
    };
    match extremum_column(value_shape) {
        ExtremumColumn::Numeric => Ok(decode_exact_aggregate(row, numeric_column)?.map(AggregationMinMax::Number)),
        ExtremumColumn::NumericAsBoolean => decode_exact_aggregate(row, numeric_column)?
            .map(|counted| boolean_extremum(counted, numeric_column))
            .transpose(),
        ExtremumColumn::Text => Ok(projected_column::<String>(row, text_column)?.map(AggregationMinMax::String)),
        ExtremumColumn::Instant => Ok(projected_column::<DateTime<Utc>>(row, instant_column)?.map(AggregationMinMax::DateTime)),
        ExtremumColumn::Unordered => Ok(None),
    }
}

/// The boolean whose count produced a numeric extremum.
///
/// The unified numeric expression counts a boolean as zero or one, so the
/// extremum arrives as one of those two numbers and is reported in the family
/// the observations were recorded in. Any other number means the row did not
/// come from that expression, which is a fault rather than a value.
fn boolean_extremum(counted: JsonNumber, column: &str) -> Result<AggregationMinMax, TemporalStoreError> {
    match counted {
        JsonNumber::Signed(0) => Ok(AggregationMinMax::Boolean(false)),
        JsonNumber::Signed(1) => Ok(AggregationMinMax::Boolean(true)),
        other => Err(TemporalStoreError::Decode(format!(
            "{column} of a boolean attribute is {other}, which counts neither false nor true"
        ))),
    }
}

/// Read one `NUMERIC` aggregate as the exact JSON number it is.
fn decode_exact_aggregate(row: &PgRow, column: &str) -> Result<Option<JsonNumber>, TemporalStoreError> {
    match projected_column::<BigDecimal>(row, column)? {
        Some(decimal) => Ok(Some(json_number_of(&decimal, column)?)),
        None => Ok(None),
    }
}

/// Read a column the statement projects only for the methods that need it.
///
/// A column the statement never selected is absent, which is what the requested
/// method list says it should be. A column that is present and unreadable is an
/// error: the alternative is reporting a computed aggregate as no aggregate at
/// all.
fn projected_column<'r, T>(row: &'r PgRow, column: &str) -> Result<Option<T>, TemporalStoreError>
where
    T: Decode<'r, Postgres> + Type<Postgres>,
{
    match row.try_column(column) {
        Err(_) => Ok(None),
        Ok(_) => row
            .try_get::<Option<T>, _>(column)
            .map_err(|error| TemporalStoreError::Decode(format!("invalid aggregation {column}: {error}"))),
    }
}

/// Read the two registry-code bounds one aggregation group spanned.
fn decode_code_span<Code>(row: &PgRow, lowest_column: &str, highest_column: &str) -> Result<CodeSpan<Code>, TemporalStoreError>
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
fn decode_code<Code>(row: &PgRow, column: &str) -> Result<Code, TemporalStoreError>
where
    Code: TryFrom<i16, Error = UnknownCompactCode>,
{
    let stored: Option<i16> = row
        .try_get(column)
        .map_err(|error| TemporalStoreError::Decode(format!("invalid aggregation {column}: {error}")))?;
    let stored = stored.ok_or_else(|| TemporalStoreError::Decode(format!("aggregation {column} is absent from a non-empty group")))?;
    Code::try_from(stored).map_err(|error| TemporalStoreError::Decode(format!("aggregation {column}: {error}")))
}

/// Read the unit identities one aggregation group's rows carried.
///
/// `MIN` and `MAX` skip nulls, so the two bounds alone cannot tell a group whose
/// rows all carried a unit from one that also held unannotated rows; the null
/// test projected beside them is what separates the two.
fn decode_observed_units(row: &PgRow) -> Result<ObservedUnits, TemporalStoreError> {
    let lowest = decode_unit_code_id(row, "unit_code_id_lowest")?;
    let highest = decode_unit_code_id(row, "unit_code_id_highest")?;
    let any_unitless: Option<bool> = row
        .try_get("any_unitless")
        .map_err(|error| TemporalStoreError::Decode(format!("invalid aggregation any_unitless: {error}")))?;
    let any_unitless = any_unitless.ok_or_else(|| TemporalStoreError::Decode("aggregation any_unitless is absent from a non-empty group".to_owned()))?;

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
        (Some(_), None) | (None, Some(_)) => Err(TemporalStoreError::Decode(
            "aggregation unit-code span reports a bound on one side only".to_owned(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use crate::aggregation_row_decode::{ExtremumColumn, boolean_extremum, extremum_column};
    use andromeda_ngsi_ld_model::{attribute::json_number::JsonNumber, temporal::AggregationMinMax};
    use andromeda_postgresql_codes::value_kind::ValueKind;

    /// The extremum is read from the column its value shape names, and a shape
    /// with no ordering has none at all. Enumerated over the whole registry so a
    /// shape added later cannot silently inherit the numeric column.
    #[test]
    fn every_value_shape_names_the_column_its_extremum_lives_in() {
        let expected = [
            (ValueKind::Number, ExtremumColumn::Numeric),
            (ValueKind::Boolean, ExtremumColumn::NumericAsBoolean),
            (ValueKind::String, ExtremumColumn::Text),
            (ValueKind::DateTime, ExtremumColumn::Instant),
            (ValueKind::Object, ExtremumColumn::Unordered),
            (ValueKind::Geometry, ExtremumColumn::Unordered),
            (ValueKind::Uri, ExtremumColumn::Unordered),
            (ValueKind::Tombstone, ExtremumColumn::Unordered),
        ];
        pretty_assertions::assert_eq!(expected.len(), ValueKind::ALL.len(), "a value shape is missing from the mapping");
        for (value_shape, column) in expected {
            pretty_assertions::assert_eq!(extremum_column(value_shape), column, "{value_shape:?}");
        }
    }

    /// An object aggregated through the unified numeric expression yields an
    /// array length, which is not one of the values that were observed, so it is
    /// reported as no extremum rather than as a number.
    #[test]
    fn an_unordered_shape_has_no_extremum_rather_than_a_length() {
        pretty_assertions::assert_eq!(extremum_column(ValueKind::Object), ExtremumColumn::Unordered);
    }

    #[test]
    fn a_boolean_extremum_comes_back_as_the_boolean_its_count_stood_for() {
        pretty_assertions::assert_eq!(
            boolean_extremum(JsonNumber::Signed(0), "agg_min").expect("zero counts false"),
            AggregationMinMax::Boolean(false)
        );
        pretty_assertions::assert_eq!(
            boolean_extremum(JsonNumber::Signed(1), "agg_max").expect("one counts true"),
            AggregationMinMax::Boolean(true)
        );
    }

    /// A number that counts neither `false` nor `true` did not come from the
    /// boolean branch of the numeric expression, so it is refused instead of
    /// being reported as one of the two.
    #[test]
    fn a_boolean_extremum_that_counts_neither_value_is_refused() {
        let error = boolean_extremum(JsonNumber::Signed(2), "agg_min").expect_err("two counts nothing");
        assert!(error.to_string().contains("agg_min"), "got {error}");
        assert!(boolean_extremum(JsonNumber::Double(0.5), "agg_min").is_err());
    }
}
