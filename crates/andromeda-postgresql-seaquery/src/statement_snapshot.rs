//! A statement as a snapshot test records it: the text the broker prepares,
//! laid out over lines, then the type and value of every numbered marker.
//!
//! The text is rendered the way [`Statement`] hands it to the driver, so a
//! Boolean and a row count appear written into it, and a snapshot fixes what
//! PostgreSQL is sent rather than what a plain sea-query render would show.
//! Laying the text out changes whitespace only, so a change of the statement
//! shows in a snapshot diff on the line of the clause that changed.

use crate::{bound_value_type::BoundValueType, statement::Statement};
use sea_query::{PostgresQueryBuilder, QueryBuilder};
use sqlformat::{Dialect, FormatOptions, Indent, QueryParams};
use std::iter;

/// The snapshot text of `statement`: its laid-out text, and below it one line
/// per numbered marker naming the value's type and the value as a SQL
/// literal.
#[must_use]
pub fn statement_snapshot(statement: &impl Statement) -> String {
    let (text, values) = statement.rendered();
    let options = FormatOptions {
        indent: Indent::Spaces(2),
        dialect: Dialect::PostgreSql,
        ..FormatOptions::default()
    };
    let markers = values.0.iter().enumerate().map(|(position, value)| {
        format!(
            "{}: {} {}",
            position + 1,
            BoundValueType::from(value),
            PostgresQueryBuilder.value_to_string(value)
        )
    });
    iter::once(sqlformat::format(&text, &QueryParams::None, &options))
        .chain(markers)
        .collect::<Vec<_>>()
        .join("\n")
}

/// Assert that the statement `$statement` matches the snapshot `$name` of the
/// calling crate, as [`statement_snapshot`] renders it.
///
/// Expands to `andromeda_architecture_checks::assert_named_snapshot!`, so the
/// snapshot is named and placed as every snapshot of the workspace is, and
/// the calling crate takes `insta` and `andromeda-architecture-checks` as
/// dev-dependencies.
#[macro_export]
macro_rules! assert_statement_snapshot {
    ($name:expr, $statement:expr $(,)?) => {
        ::andromeda_architecture_checks::assert_named_snapshot!($name, $crate::statement_snapshot::statement_snapshot(&$statement))
    };
}

#[cfg(test)]
mod tests {
    use crate::statement_snapshot::statement_snapshot;
    use pretty_assertions::assert_eq;
    use sea_query::{Alias, Expr, ExprTrait, Query};

    #[test]
    fn a_statement_is_laid_out_over_lines_and_followed_by_its_markers() {
        let select = Query::select()
            .column(Alias::new("id"))
            .from(Alias::new("entities"))
            .and_where(Expr::col(Alias::new("id")).eq("urn:a"))
            .and_where(Expr::col(Alias::new("rank")).gt(7))
            .to_owned();
        assert_eq!(
            statement_snapshot(&select),
            "SELECT\n  \"id\"\nFROM\n  \"entities\"\nWHERE\n  \"id\" = $1\n  AND \"rank\" > $2\n1: String 'urn:a'\n2: Int 7"
        );
    }

    #[test]
    fn a_boolean_and_a_row_count_appear_in_the_text_as_the_driver_is_sent_them() {
        let select = Query::select()
            .column(Alias::new("id"))
            .from(Alias::new("entities"))
            .and_where(Expr::col(Alias::new("deleted")).eq(false))
            .limit(5)
            .to_owned();
        assert_eq!(
            statement_snapshot(&select),
            "SELECT\n  \"id\"\nFROM\n  \"entities\"\nWHERE\n  \"deleted\" = FALSE\nLIMIT\n  5"
        );
    }
}
