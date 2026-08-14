//! Typed bind values and the conversion from sea-query's erased values.

use chrono::{DateTime, Utc};
use sea_query::{Value, Values};
use std::borrow::Cow;

/// A sea-query value had no typed [`BindValue`] counterpart, so binding it would
/// have changed the comparison semantics of the predicate.
///
/// The one failure this crate raises: every other value maps to a native column
/// type, and a value that did not would silently be rendered as text.
#[derive(Debug, thiserror::Error)]
#[error("unexpected sea-query value in bind conversion: {0}")]
pub struct BindConversionError(pub String);

/// A typed bind value for parameterized SQL queries.
///
/// The typed variants exist so `q`-filter predicates can be compared against
/// `current_attribute_values`'s native columns (`numeric`, `boolean`,
/// `timestamptz`, `integer`) instead of being rendered as text and cast in
/// SQL. A text comparison is neither index-eligible nor semantically correct
/// for timestamps, where lexicographic order diverges from temporal order
/// whenever offsets differ.
#[derive(Debug, Clone)]
pub enum BindValue<'a> {
    /// A single string or text value.
    String(Cow<'a, str>),
    /// An array of strings (for `= ANY($N)` conditions).
    StringVec(Cow<'a, [String]>),
    /// A 64-bit integer value.
    I64(i64),
    /// A 32-bit integer value; binds as PostgreSQL `integer`.
    ///
    /// Used for `attribute_names.id` dictionary keys.
    I32(i32),
    /// A 16-bit integer value; binds as PostgreSQL `smallint`.
    ///
    /// Used for the `smallint` columns the broker owns, such as a snapshot's
    /// priority.
    I16(i16),
    /// A double-precision float; binds as PostgreSQL `double precision`.
    F64(f64),
    /// A boolean value; binds as PostgreSQL `boolean`.
    Bool(bool),
    /// An instant; binds as PostgreSQL `timestamptz`.
    Timestamp(DateTime<Utc>),
    /// A byte buffer (binds as PostgreSQL `bytea`), used for EWKB geometry.
    Bytes(Cow<'a, [u8]>),
}

/// Collects bind values for a hand-written statement and hands back the `$N`
/// marker that refers to each.
///
/// The statements that need this are the ones assembled as SQL text: the
/// `lastN` ranking CTE and the `time_bucket` aggregation, which bind
/// [`BindValue`] directly, and the `q`-filter fragments handed to
/// `sea_query::Expr::cust_with_values`, which bind [`sea_query::Value`]. The
/// value type is a parameter because the numbering rule is the same either way
/// and was previously written out once per value type.
///
/// Numbering by hand is what this replaces: it used to be arithmetic three
/// functions agreed on, with the entity id's `$1` and the 1-based offset
/// re-derived as `len() + 2` in two separate places and nothing checking that
/// the result was contiguous.
#[derive(Debug)]
pub struct BindBuilder<V> {
    /// Values collected so far, in placeholder order.
    values: Vec<V>,
}

// `#[derive(Default)]` would demand `V: Default`, which no value type here
// satisfies and none needs: an empty builder holds no value at all.
impl<V> Default for BindBuilder<V> {
    fn default() -> BindBuilder<V> {
        BindBuilder { values: Vec::new() }
    }
}

impl<V> BindBuilder<V> {
    /// Collect one value and return the `$N` marker that refers to it.
    pub fn bind(&mut self, value: impl Into<V>) -> String {
        self.values.push(value.into());
        format!("${}", self.values.len())
    }

    /// The collected values, in the order their markers were handed out.
    pub fn into_values(self) -> Vec<V> {
        self.values
    }
}

/// Convert sea-query's erased [`Values`] into typed [`BindValue`]s.
///
/// Shared by every SQL builder so the mapping stays consistent: a value bound
/// as a float in one builder and as text in another would silently change the
/// comparison semantics of the generated predicate.
pub fn sea_values_to_bind_values(values: Values) -> Result<Vec<BindValue<'static>>, BindConversionError> {
    values
        .0
        .into_iter()
        .map(|value| match value {
            Value::String(Some(s)) => Ok(BindValue::String(Cow::Owned(s.to_string()))),
            // `int4` stays `int4`: the dictionary key is the only value bound
            // this way, and widening it to `int8` would make every
            // `attribute_name_id = $N` comparison cross-type.
            Value::Int(Some(i)) => Ok(BindValue::I32(i)),
            Value::BigInt(Some(i)) => Ok(BindValue::I64(i)),
            Value::BigUnsigned(Some(u)) => Ok(BindValue::I64(u as i64)),
            Value::Unsigned(Some(u)) => Ok(BindValue::I64(i64::from(u))),
            Value::TinyInt(Some(i)) => Ok(BindValue::I64(i64::from(i))),
            Value::TinyUnsigned(Some(u)) => Ok(BindValue::I64(i64::from(u))),
            Value::SmallInt(Some(i)) => Ok(BindValue::I16(i)),
            Value::SmallUnsigned(Some(u)) => Ok(BindValue::I64(i64::from(u))),
            Value::Bool(Some(b)) => Ok(BindValue::Bool(b)),
            Value::Double(Some(f)) => Ok(BindValue::F64(f)),
            Value::Float(Some(f)) => Ok(BindValue::F64(f64::from(f))),
            Value::ChronoDateTimeUtc(Some(dt)) => Ok(BindValue::Timestamp(*dt)),
            Value::Bytes(Some(b)) => Ok(BindValue::Bytes(Cow::Owned(*b))),
            other => Err(BindConversionError(format!("{other:?}"))),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    /// Each sea-query variant has to reach the typed `BindValue` that binds it
    /// natively. Rendering a float or a timestamp as text would silently change
    /// the comparison semantics of every predicate that uses it.
    #[test]
    fn typed_sea_query_values_map_to_typed_bind_values() {
        let values = Values(vec![
            Value::SmallInt(Some(7)),
            Value::Int(Some(4242)),
            Value::Double(Some(50.5)),
            Value::Bool(Some(true)),
            Value::ChronoDateTimeUtc(Some(Box::new(Utc.timestamp_opt(1_700_000_000, 0).single().expect("timestamp")))),
            Value::String(Some(Box::new("text".to_string()))),
            Value::BigInt(Some(42)),
        ]);
        let binds = sea_values_to_bind_values(values).expect("bind conversion");
        assert!(matches!(binds[0], BindValue::I16(7)), "{binds:?}");
        assert!(matches!(binds[1], BindValue::I32(4242)), "int4 must not widen to int8: {binds:?}");
        assert!(matches!(binds[2], BindValue::F64(value) if (value - 50.5).abs() < f64::EPSILON), "{binds:?}");
        assert!(matches!(binds[3], BindValue::Bool(true)), "{binds:?}");
        assert!(matches!(binds[4], BindValue::Timestamp(_)), "{binds:?}");
        assert!(matches!(&binds[5], BindValue::String(text) if text == "text"), "{binds:?}");
        assert!(matches!(binds[6], BindValue::I64(42)), "{binds:?}");
    }

    #[test]
    fn markers_are_handed_out_contiguously_from_one() {
        let mut binds: BindBuilder<BindValue<'static>> = BindBuilder::default();
        assert_eq!(binds.bind(BindValue::I64(1)), "$1");
        assert_eq!(binds.bind(BindValue::Bool(true)), "$2");
        assert_eq!(binds.bind(BindValue::F64(1.5)), "$3");
        assert_eq!(binds.into_values().len(), 3);
    }

    /// The same numbering rule serves the sea-query fragments, which bind
    /// `sea_query::Value` rather than [`BindValue`].
    #[test]
    fn the_numbering_rule_is_shared_by_both_value_types() {
        let mut binds: BindBuilder<Value> = BindBuilder::default();
        assert_eq!(binds.bind(7_i16), "$1");
        assert_eq!(binds.bind("text"), "$2");
        assert_eq!(binds.into_values().len(), 2);
    }

    #[test]
    fn a_float_binds_as_a_float_not_as_text() {
        // Regression guard: the previous converter rendered Double as a string,
        // which made every numeric q predicate a text comparison.
        let values = Values(vec![Value::Float(Some(1.5))]);
        let binds = sea_values_to_bind_values(values).expect("bind conversion");
        assert!(matches!(binds[0], BindValue::F64(_)), "{binds:?}");
    }
}
