//! The `[value, start, end]` triples the aggregation rows of one attribute
//! instance make, nested under the grouping terms.

use crate::temporal::{
    AggregationAverage,
    AggregationMinMax,
    GroupKeyValue,
    TemporalAggregationRow,
    aggregated_entity::{AggregateValue, AggregatedTriple, AggregatedValues},
};
use andromeda_ngsi_ld_lexicon::{aggr_method::AggrMethod, time::value::TimeValue};
use andromeda_ngsi_ld_number::JsonNumber;
use indexmap::IndexMap;

/// The values of one method over a set of rows, nested under one map key per
/// grouping term.
///
/// At each level the rows are partitioned by their group-key value at that
/// level, preserving the order the store returned them in (clause 4.23.2 plus
/// the query's collation, applied by the SQL `ORDER BY`). The leaf, once every
/// term has been consumed, is the triple array the standard format emits.
pub(crate) fn nested(rows: &[&TemporalAggregationRow], method: AggrMethod, level: usize, depth: usize) -> AggregatedValues {
    if level == depth {
        return AggregatedValues::Triples(rows.iter().map(|row| triple(row, method)).collect());
    }
    let mut buckets: IndexMap<GroupKeyValue, Vec<&TemporalAggregationRow>> = IndexMap::new();
    for row in rows {
        let key = row.group_keys.get(level).cloned().unwrap_or(GroupKeyValue::Absent); // clone: the bucket map owns its keys
        buckets.entry(key).or_default().push(*row);
    }
    AggregatedValues::Grouped(
        buckets
            .into_iter()
            .map(|(key, sub_rows)| (key, nested(&sub_rows, method, level + 1, depth)))
            .collect(),
    )
}

/// The `[value, start, end]` triple one method contributes over one row.
fn triple(row: &TemporalAggregationRow, method: AggrMethod) -> AggregatedTriple {
    let value = match method {
        AggrMethod::TotalCount => row.total_count.map_or(AggregateValue::Absent, AggregateValue::Count),
        AggrMethod::DistinctCount => row.distinct_count.map_or(AggregateValue::Absent, AggregateValue::Count),
        AggrMethod::Min => row.min.as_ref().map_or(AggregateValue::Absent, extremum),
        AggrMethod::Max => row.max.as_ref().map_or(AggregateValue::Absent, extremum),
        AggrMethod::Avg => row.avg.as_ref().map_or(AggregateValue::Absent, average),
        AggrMethod::Sum => number(row.sum.as_ref()),
        AggrMethod::SumSq => number(row.sumsq.as_ref()),
        AggrMethod::StdDev => number(row.stddev.as_ref()),
    };
    AggregatedTriple {
        value,
        start: row.bucket_start,
        end: row.bucket_end,
    }
}

/// An exact number, or nothing.
fn number(value: Option<&JsonNumber>) -> AggregateValue {
    value.map_or(AggregateValue::Absent, |number| AggregateValue::Number(number.clone()))
}

/// A typed minimum or maximum, in the family it was observed in.
///
/// The numeric extremum keeps its own JSON-number shape, so an integer beyond
/// 2^53 is the integer and not the double nearest to it.
fn extremum(value: &AggregationMinMax) -> AggregateValue {
    match value {
        AggregationMinMax::Number(number) => AggregateValue::Number(number.clone()),
        AggregationMinMax::String(text) => AggregateValue::Text(text.clone()),
        AggregationMinMax::Boolean(flag) => AggregateValue::Boolean(*flag),
        AggregationMinMax::DateTime(instant) => AggregateValue::Instant(*instant),
        AggregationMinMax::Time(time) => AggregateValue::Text(TimeValue::Time(*time).to_string()),
    }
}

/// An arithmetic mean: Table 4.5.19.1-2 defines `avg` over a `Time` as "the
/// average time inside the period", so it is in the clause 4.6.3 `Time`
/// spelling the broker serves a time of day in, rather than the number of
/// seconds it was computed from.
fn average(value: &AggregationAverage) -> AggregateValue {
    match value {
        AggregationAverage::Number(number) => AggregateValue::Number(number.clone()),
        AggregationAverage::Time(time) => AggregateValue::Text(TimeValue::Time(*time).to_string()),
    }
}
