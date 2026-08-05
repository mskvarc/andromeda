//! The output of SQL generation.

use crate::sql::bind::BindValue;

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
