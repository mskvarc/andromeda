//! Bare positional parameter markers for values sea-query cannot carry.
//!
//! Almost every value in a converted statement is a sea-query bound value:
//! `build_sqlx` renders its `$n` marker and numbers it. One value shape cannot
//! travel that way — a nullable-element array (`Vec<Option<T>>`) — because
//! `sea-query-sqlx` cannot carry it in either direction:
//!
//! - Reading a `Value::Array` back into a Postgres array binds it as
//!   `Option<Vec<T>>`, and the `Value` to `Vec<T>` conversion unwraps every
//!   element (`e.unwrap()`), so a NULL element panics at execute time.
//! - A `Value::Array` cannot even be built from a `Vec<Option<T>>`: the
//!   `From<Vec<T>> for Value` impl requires `T: NotU8 + ValueType`, which
//!   `Option<T>` does not satisfy, so there is no `Value` to bind.
//!
//! `sqlx` binds `Vec<Option<T>>` natively. For a statement whose only bound
//! values are such arrays, the structure is still built by sea-query — every
//! relation, column, cast and clause — while the array arguments are emitted as
//! bare `$n` markers by [`external_placeholder`] and bound by the store through
//! `sqlx` in marker order. No repository spells the statement text; only the one
//! crate sanctioned to name a dialect token spells the marker.
//!
//! A statement that binds every value outside sea-query this way casts a marker
//! whose bound text the server converts, such as JSON text stored as `json`, with
//! [`external_placeholder_as`].

use crate::{cast::cast, pg_type::PostgresType};
use sea_query::{Expr, SimpleExpr};

/// A bare positional parameter marker `$index`, bound by the caller outside
/// sea-query.
///
/// The marker carries no value, so sea-query numbers nothing: the caller is
/// responsible for binding a value at `index` and for every marker in the
/// statement being one of these, so the numbering stays contiguous. This exists
/// only for the nullable-element array case the module documents; a value
/// sea-query can carry is bound the ordinary way instead.
#[must_use]
pub fn external_placeholder(index: usize) -> SimpleExpr {
    Expr::cust(format!("${index}"))
}

/// A bare positional parameter marker cast to `ty`, rendered `$index::ty`: the
/// caller binds the value's text and the server converts it.
#[must_use]
pub fn external_placeholder_as(index: usize, ty: PostgresType) -> SimpleExpr {
    cast(external_placeholder(index), ty)
}

#[cfg(test)]
mod tests {
    use crate::{
        pg_type::PostgresType,
        placeholder::{external_placeholder, external_placeholder_as},
    };
    use pretty_assertions::assert_eq;
    use sea_query::{PostgresQueryBuilder, Query};

    fn render(expr: sea_query::SimpleExpr) -> String {
        Query::select().expr(expr).to_owned().to_string(PostgresQueryBuilder)
    }

    #[test]
    fn a_placeholder_renders_its_marker_and_binds_nothing() {
        let (sql, values) = Query::select().expr(external_placeholder(7)).to_owned().build(PostgresQueryBuilder);
        assert!(sql.contains("$7"), "{sql}");
        assert_eq!(values.0.len(), 0, "the marker carries no bound value: {sql}");
    }

    #[test]
    fn the_index_is_the_marker_number() {
        assert!(render(external_placeholder(1)).contains("$1"));
        assert!(render(external_placeholder(14)).contains("$14"));
    }

    #[test]
    fn a_cast_placeholder_renders_its_marker_and_the_type_it_converts_to() {
        assert_eq!(render(external_placeholder_as(4, PostgresType::Json)), "SELECT $4::json");
    }
}
