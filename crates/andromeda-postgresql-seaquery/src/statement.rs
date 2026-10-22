//! Handing a statement with bound values to the driver.
//!
//! Every statement the broker runs reaches PostgreSQL through [`Statement`] or
//! [`crate::command::Command`]: a sea-query statement, or one this crate
//! spells for what sea-query has no form for. Both traits are sealed and their
//! rendering is private to this crate, so statement text is produced and
//! consumed here and never travels as a string through another crate: a caller
//! holds a statement value and takes the driver query from it.
//!
//! A statement runs on the extended protocol: its text is prepared once per
//! connection and its values travel as arguments, never inside the text.
//!
//! sea-query is built thread-safe, so a statement value is `Send` and may be
//! held across the `await` that runs it.

use crate::statement_arguments::statement_arguments;
use sea_query::{DeleteStatement, InsertStatement, PostgresQueryBuilder, SelectStatement, UpdateStatement, Values, WithQuery};
use sqlx::{
    AssertSqlSafe,
    FromRow,
    Postgres,
    postgres::{PgArguments, PgRow},
    query::{Query, QueryAs, QueryScalar},
};
use sqlx_core::{query::query_with_result, query_as::query_as_with_result, query_scalar::query_scalar_with_result};

/// The rendering every statement of this crate shares, out of reach of other
/// crates so no statement text leaves this one.
pub(crate) mod rendering {
    use sea_query::Values;

    /// A statement's text, its markers numbered `$1..`, and the values those
    /// markers stand for, in marker order.
    pub trait Rendered {
        /// The text and the values of the statement.
        fn rendered(&self) -> (String, Values);
    }
}

/// A statement whose values travel as bound arguments.
pub trait Statement: rendering::Rendered + Send + Sync {
    /// The driver query running the statement.
    ///
    /// A value that cannot be encoded fails the query when it runs.
    fn to_query(&self) -> Query<'static, Postgres, PgArguments> {
        let (sql, values) = self.rendered();
        query_with_result(AssertSqlSafe(sql), statement_arguments(values))
    }

    /// The driver query running the statement, each row read as an `O`.
    fn to_query_as<O>(&self) -> QueryAs<'static, Postgres, O, PgArguments>
    where
        O: for<'row> FromRow<'row, PgRow>,
    {
        let (sql, values) = self.rendered();
        query_as_with_result(AssertSqlSafe(sql), statement_arguments(values))
    }

    /// The driver query running the statement, the first column of each row
    /// read as an `O`.
    fn to_query_scalar<O>(&self) -> QueryScalar<'static, Postgres, O, PgArguments>
    where
        (O,): for<'row> FromRow<'row, PgRow>,
    {
        let (sql, values) = self.rendered();
        query_scalar_with_result(AssertSqlSafe(sql), statement_arguments(values))
    }
}

/// Render and mark as a [`Statement`] each sea-query statement type the broker
/// runs.
macro_rules! sea_query_statement {
    ($($statement:ty),+ $(,)?) => {
        $(
            impl rendering::Rendered for $statement {
                fn rendered(&self) -> (String, Values) {
                    self.build(PostgresQueryBuilder)
                }
            }

            impl Statement for $statement {}
        )+
    };
}

sea_query_statement!(SelectStatement, InsertStatement, UpdateStatement, DeleteStatement, WithQuery);

#[cfg(test)]
mod tests {
    use crate::statement::rendering::Rendered;
    use sea_query::{Alias, Expr, ExprTrait, Query, Value};

    #[test]
    fn a_select_renders_its_values_as_numbered_markers() {
        let (sql, values) = Query::select()
            .column(Alias::new("id"))
            .from(Alias::new("entities"))
            .and_where(Expr::col(Alias::new("id")).eq("urn:a"))
            .to_owned()
            .rendered();
        pretty_assertions::assert_eq!(sql, r#"SELECT "id" FROM "entities" WHERE "id" = $1"#);
        pretty_assertions::assert_eq!(values.0, vec![Value::String(Some("urn:a".to_owned()))]);
    }
}
