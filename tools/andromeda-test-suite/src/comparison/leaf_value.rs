//! The comparison of the values the suite compares by their meaning rather than their text: an `observedAt` instant,
//! a system timestamp and a number.

use crate::{
    comparison::{
        Mode,
        difference::{Difference, Discrepancy},
        document_path::DocumentPath,
    },
    ngsild_date::same_instant,
};
use chrono::DateTime;
use serde_json::{Number, Value};

/// Compares an `observedAt` as an instant.
pub(crate) fn compare_observed_at(path: &DocumentPath, actual: &Value, expected: &Value, out: &mut Vec<Difference>) {
    match (actual, expected) {
        (Value::String(actual), Value::String(expected)) if same_instant(actual, expected) => {}
        (actual, expected) => out.push(Difference::at(
            path,
            Discrepancy::OtherInstant {
                actual: actual.clone(), // clone: the difference owns the values it reports
                expected: expected.clone(),
            },
        )),
    }
}

/// Requires a system timestamp to be a well-formed date.
pub(crate) fn compare_system_timestamp(path: &DocumentPath, actual: &Value, out: &mut Vec<Difference>) {
    if actual.as_str().is_none_or(|text| DateTime::parse_from_rfc3339(text).is_err()) {
        out.push(Difference::at(path, Discrepancy::MalformedDate(actual.clone()))); // clone: the difference owns the values it reports
    }
}

/// Compares two numbers: by value and kind in a diff, by value in a hash.
pub(crate) fn compare_numbers(path: &DocumentPath, actual: &Number, expected: &Number, mode: Mode, out: &mut Vec<Difference>) {
    let same_value = match (actual.as_i64(), expected.as_i64(), actual.as_u64(), expected.as_u64()) {
        (Some(actual), Some(expected), _, _) => actual == expected,
        (_, _, Some(actual), Some(expected)) => actual == expected,
        _ => actual
            .as_f64()
            .zip(expected.as_f64())
            .is_some_and(|(actual, expected)| actual.total_cmp(&expected).is_eq()),
    };
    let same_kind = actual.is_f64() == expected.is_f64();
    match (same_value, same_kind, mode) {
        (true, true, _) | (true, false, Mode::Hash) => {}
        (true, false, Mode::Diff) => out.push(Difference::at(
            path,
            Discrepancy::OtherNumberType {
                actual: actual.clone(), // clone: the difference owns the values it reports
                expected: expected.clone(),
            },
        )),
        (false, _, _) => out.push(Difference::at(
            path,
            Discrepancy::Unequal {
                actual: Value::Number(actual.clone()), // clone: the difference owns the values it reports
                expected: Value::Number(expected.clone()),
            },
        )),
    }
}
