//! SQL generation from compiled query types.
//!
//! Shared types, bind helpers, and utility functions used across entity,
//! temporal, and CSR SQL generators. Uses sea-query for type-safe query building.

pub mod csr;
pub mod entity;
pub mod postgres_ext;
/// Shared `q`-filter predicate compiler used by the entity and temporal builders.
pub mod q_filter;
pub mod schema;
pub mod temporal;

use crate::core::error::AndromedaError;
use chrono::{DateTime, Utc};
use std::borrow::Cow;

/// The result of SQL query generation: a parameterized SQL string plus bind values.
#[derive(Debug)]
pub struct BuiltQuery<'a> {
    /// The parameterized SQL query string with `$N` placeholders.
    pub sql: String,
    /// Optional count query for pagination (when `count=true`).
    pub count_sql: Option<String>,
    /// Bind values corresponding to the `$N` placeholders in order.
    pub bind_values: Vec<BindValue<'a>>,
}

/// Bind all values in a [`BindValue`] slice to a sqlx query.
///
/// Works with `sqlx::query()`, `sqlx::query_scalar()`, and `sqlx::query_as()` since
/// all share the same `.bind()` method signature. The query type is returned after
/// all binds are applied.
#[macro_export]
macro_rules! bind_values {
    ($query:expr, $binds:expr) => {{
        let mut q = $query;
        for bind in $binds {
            q = match bind {
                $crate::query::sql::BindValue::String(s) => q.bind(s.as_ref()),
                $crate::query::sql::BindValue::StringVec(v) => q.bind(v.as_ref()),
                $crate::query::sql::BindValue::I64(i) => q.bind(*i),
                $crate::query::sql::BindValue::I16(i) => q.bind(*i),
                $crate::query::sql::BindValue::F64(f) => q.bind(*f),
                $crate::query::sql::BindValue::Bool(b) => q.bind(*b),
                $crate::query::sql::BindValue::Timestamp(t) => q.bind(*t),
                $crate::query::sql::BindValue::Bytes(b) => q.bind(b.as_ref()),
            };
        }
        q
    }};
}

/// A typed bind value for parameterized SQL queries.
///
/// The typed variants exist so `q`-filter predicates can be compared against
/// `attribute_current`'s native columns (`double precision`, `boolean`,
/// `timestamptz`, `smallint`) instead of being rendered as text and cast in
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
    /// A 16-bit integer value; binds as PostgreSQL `smallint`.
    ///
    /// Used for `attribute_names.id` dictionary keys.
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

/// Convert sea-query's erased [`sea_query::Values`] into typed [`BindValue`]s.
///
/// Shared by every SQL builder so the mapping stays consistent: a value bound
/// as a float in one builder and as text in another would silently change the
/// comparison semantics of the generated predicate.
pub fn sea_values_to_bind_values(values: sea_query::Values) -> Result<Vec<BindValue<'static>>, AndromedaError> {
    values
        .0
        .into_iter()
        .map(|value| match value {
            sea_query::Value::String(Some(s)) => Ok(BindValue::String(Cow::Owned(s.to_string()))),
            sea_query::Value::Int(Some(i)) => Ok(BindValue::I64(i64::from(i))),
            sea_query::Value::BigInt(Some(i)) => Ok(BindValue::I64(i)),
            sea_query::Value::BigUnsigned(Some(u)) => Ok(BindValue::I64(u as i64)),
            sea_query::Value::Unsigned(Some(u)) => Ok(BindValue::I64(i64::from(u))),
            sea_query::Value::TinyInt(Some(i)) => Ok(BindValue::I64(i64::from(i))),
            sea_query::Value::TinyUnsigned(Some(u)) => Ok(BindValue::I64(i64::from(u))),
            sea_query::Value::SmallInt(Some(i)) => Ok(BindValue::I16(i)),
            sea_query::Value::SmallUnsigned(Some(u)) => Ok(BindValue::I64(i64::from(u))),
            sea_query::Value::Bool(Some(b)) => Ok(BindValue::Bool(b)),
            sea_query::Value::Double(Some(f)) => Ok(BindValue::F64(f)),
            sea_query::Value::Float(Some(f)) => Ok(BindValue::F64(f64::from(f))),
            sea_query::Value::ChronoDateTimeUtc(Some(dt)) => Ok(BindValue::Timestamp(*dt)),
            sea_query::Value::Bytes(Some(b)) => Ok(BindValue::Bytes(Cow::Owned(*b))),
            other => Err(AndromedaError::internal(format!(
                "unexpected sea_query::Value variant in bind conversion: {other:?}"
            ))),
        })
        .collect()
}

/// Convert an NGSI-LD scope path to PostgreSQL ltree format for storage.
///
/// Strips the leading `/` and replaces remaining `/` separators with `.` to form
/// a valid ltree label path.
///
/// # Examples
/// - `/A/B/C` -> `A.B.C`
/// - `/Madrid` -> `Madrid`
pub fn scope_to_ltree(scope: &str) -> String {
    scope.trim_start_matches('/').replace('/', ".")
}

/// Convert an NGSI-LD scope query pattern to PostgreSQL lquery format for matching.
///
/// Translates NGSI-LD wildcard conventions into lquery syntax:
/// - `#` (at end) becomes `*{1,}` (one or more labels)
/// - `+` becomes `*{1}` (exactly one label)
/// - Literal segments pass through unchanged.
///
/// # Examples
/// - `/A/B/C` -> `A.B.C`
/// - `/A/#` -> `A.*{1,}`
/// - `/A/+/C` -> `A.*{1}.C`
/// - `/#` -> `*{1,}`
pub fn scope_to_lquery(scope: &str) -> String {
    let trimmed = scope.trim_start_matches('/');
    trimmed
        .split('/')
        .map(|segment| match segment {
            "#" => "*{1,}",
            "+" => "*{1}",
            other => other,
        })
        .collect::<Vec<_>>()
        .join(".")
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]
    use super::*;
    use chrono::TimeZone;

    /// Each sea-query variant has to reach the typed `BindValue` that binds it
    /// natively. Rendering a float or a timestamp as text would silently change
    /// the comparison semantics of every predicate that uses it.
    #[test]
    fn typed_sea_query_values_map_to_typed_bind_values() {
        let values = sea_query::Values(vec![
            sea_query::Value::SmallInt(Some(7)),
            sea_query::Value::Double(Some(50.5)),
            sea_query::Value::Bool(Some(true)),
            sea_query::Value::ChronoDateTimeUtc(Some(Box::new(Utc.timestamp_opt(1_700_000_000, 0).single().expect("timestamp")))),
            sea_query::Value::String(Some(Box::new("text".to_string()))),
            sea_query::Value::BigInt(Some(42)),
        ]);
        let binds = sea_values_to_bind_values(values).expect("bind conversion");
        assert!(matches!(binds[0], BindValue::I16(7)), "{binds:?}");
        assert!(matches!(binds[1], BindValue::F64(value) if (value - 50.5).abs() < f64::EPSILON), "{binds:?}");
        assert!(matches!(binds[2], BindValue::Bool(true)), "{binds:?}");
        assert!(matches!(binds[3], BindValue::Timestamp(_)), "{binds:?}");
        assert!(matches!(&binds[4], BindValue::String(text) if text == "text"), "{binds:?}");
        assert!(matches!(binds[5], BindValue::I64(42)), "{binds:?}");
    }

    #[test]
    fn a_float_binds_as_a_float_not_as_text() {
        // Regression guard: the previous converter rendered Double as a string,
        // which made every numeric q predicate a text comparison.
        let values = sea_query::Values(vec![sea_query::Value::Float(Some(1.5))]);
        let binds = sea_values_to_bind_values(values).expect("bind conversion");
        assert!(matches!(binds[0], BindValue::F64(_)), "{binds:?}");
    }
}
