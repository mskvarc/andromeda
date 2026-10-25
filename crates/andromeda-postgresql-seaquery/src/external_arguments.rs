//! Arguments a statement carries in an encoding of their own.
//!
//! A statement value travels as a sea-query value, numbered and bound by
//! [`crate::statement`]. A value sea-query has no form for (an array of a
//! composite type, encoded field for field by the type's own binary encoding)
//! travels instead as an external argument: [`ExternalArguments::push`]
//! returns the expression standing for it in the statement, and the argument
//! is bound after the statement's own values, in push order, its marker
//! numbered to match wherever the expression was placed. No caller numbers a
//! marker or binds in a particular order.

use crate::{statement::rendering::Rendered, statement_arguments::statement_arguments};
use sea_query::{Expr, SimpleExpr};
use sqlx::{
    Arguments,
    AssertSqlSafe,
    Encode,
    FromRow,
    Postgres,
    Type,
    error::BoxDynError,
    postgres::{PgArguments, PgRow},
    query::{Query, QueryAs, QueryScalar},
};
use sqlx_core::{query::query_with_result, query_as::query_as_with_result, query_scalar::query_scalar_with_result};

/// Binds one external argument as the next driver argument.
type Binder<'value> = Box<dyn FnOnce(&mut PgArguments) -> Result<(), BoxDynError> + Send + 'value>;

/// The external arguments of one statement, in push order.
#[derive(Default)]
pub struct ExternalArguments<'value> {
    /// One binder per argument.
    binders: Vec<Binder<'value>>,
}

impl<'value> ExternalArguments<'value> {
    /// Carry `value` as the next external argument, and return the
    /// expression standing for it in the statement.
    pub fn push<T>(&mut self, value: T) -> SimpleExpr
    where
        T: Encode<'value, Postgres> + Type<Postgres> + Send + 'value,
    {
        let marker = Expr::cust(marker(self.binders.len()));
        self.binders.push(Box::new(move |arguments: &mut PgArguments| arguments.add(value)));
        marker
    }
}

/// The text standing for external argument `index` until the statement is
/// rendered.
///
/// It is delimited by NUL characters, which no identifier, literal or
/// keyword of a PostgreSQL statement can hold, so it cannot collide with
/// anything else the statement says.
fn marker(index: usize) -> String {
    format!("\u{0}{index}\u{0}")
}

/// A statement together with the external arguments its markers stand for.
pub struct ExternallyBound<'value, S> {
    /// The statement, its external arguments placed as markers.
    statement: S,
    /// The arguments the markers stand for.
    arguments: ExternalArguments<'value>,
}

impl<'value, S: Rendered> ExternallyBound<'value, S> {
    /// `statement`, whose markers stand for `arguments`.
    pub const fn new(statement: S, arguments: ExternalArguments<'value>) -> ExternallyBound<'value, S> {
        ExternallyBound { statement, arguments }
    }

    /// The statement text with every marker numbered after the statement's
    /// own values, and every argument bound, external ones last.
    fn bound(self) -> (String, Result<PgArguments, BoxDynError>) {
        let (mut sql, values) = self.statement.rendered();
        let own = values.0.len();
        for index in 0..self.arguments.binders.len() {
            sql = sql.replace(&marker(index), &format!("${}", own + index + 1));
        }
        let arguments = statement_arguments(values)
            .and_then(|mut arguments| self.arguments.binders.into_iter().try_for_each(|bind| bind(&mut arguments)).map(|()| arguments));
        (sql, arguments)
    }

    /// The driver query running the statement.
    ///
    /// A value that cannot be encoded fails the query when it runs.
    pub fn into_query(self) -> Query<'static, Postgres, PgArguments> {
        let (sql, arguments) = self.bound();
        query_with_result(AssertSqlSafe(sql), arguments)
    }

    /// The driver query running the statement, each row read as an `O`.
    pub fn into_query_as<O>(self) -> QueryAs<'static, Postgres, O, PgArguments>
    where
        O: for<'row> FromRow<'row, PgRow>,
    {
        let (sql, arguments) = self.bound();
        query_as_with_result(AssertSqlSafe(sql), arguments)
    }

    /// The driver query running the statement, the first column of each row
    /// read as an `O`.
    pub fn into_query_scalar<O>(self) -> QueryScalar<'static, Postgres, O, PgArguments>
    where
        (O,): for<'row> FromRow<'row, PgRow>,
    {
        let (sql, arguments) = self.bound();
        query_scalar_with_result(AssertSqlSafe(sql), arguments)
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::external_arguments::{ExternalArguments, ExternallyBound};
    use sea_query::{Alias, Expr, ExprTrait, Query};

    #[test]
    fn external_markers_are_numbered_after_the_statements_own_values() {
        let mut external = ExternalArguments::default();
        let records = external.push(vec![1_i64, 2]);
        let statement = Query::select()
            .expr(records)
            .from(Alias::new("t"))
            .and_where(Expr::col(Alias::new("id")).eq("a"))
            .to_owned();
        let (sql, arguments) = ExternallyBound::new(statement, external).bound();
        pretty_assertions::assert_eq!(sql, r#"SELECT $2 FROM "t" WHERE "id" = $1"#);
        pretty_assertions::assert_eq!(sqlx::Arguments::len(&arguments.expect("bindable arguments")), 2);
    }
}
