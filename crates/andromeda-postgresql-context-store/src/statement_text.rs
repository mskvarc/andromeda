//! The text of a statement as the driver receives it, for tests that assert on
//! what an alias rewrite or a marker numbering produced.

use andromeda_postgresql_seaquery::statement::Statement;
use sqlx::{Execute, Postgres, postgres::PgArguments, query::Query};

/// The statement text of `query`, its markers numbered as the server sees them.
pub(crate) fn text_of_query(query: Query<'_, Postgres, PgArguments>) -> String {
    query.sql().as_str().to_owned()
}

/// The statement text of `statement`, its values bound as markers.
pub(crate) fn text_of(statement: &impl Statement) -> String {
    text_of_query(statement.to_query())
}
