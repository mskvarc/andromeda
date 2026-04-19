//! SQL generation from compiled query types.
//!
//! Shared types, bind helpers, and utility functions used across entity,
//! temporal, and CSR SQL generators. Uses sea-query for type-safe query building.

pub mod csr;
pub mod entity;
pub mod postgres_ext;
pub mod schema;
pub mod temporal;

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
                $crate::query::sql::BindValue::Bytes(b) => q.bind(b.as_ref()),
            };
        }
        q
    }};
}

/// A typed bind value for parameterized SQL queries.
#[derive(Debug, Clone)]
pub enum BindValue<'a> {
    /// A single string or text value.
    String(Cow<'a, str>),
    /// An array of strings (for `= ANY($N)` conditions).
    StringVec(Cow<'a, [String]>),
    /// A 64-bit integer value.
    I64(i64),
    /// A byte buffer (binds as PostgreSQL `bytea`), used for EWKB geometry.
    Bytes(Cow<'a, [u8]>),
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
